//! Loopback-only, server-rendered OpenWAM. All CAM work remains in its owners.
use crate::{MAX_INPUT_BYTES, bridge, cutting, openwam};
use anyhow::{Context, Result, ensure};
use axum::{
    Form, Router,
    extract::{DefaultBodyLimit, Multipart, Path, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fmt::Write,
    sync::{Arc, Mutex},
};
use tokio::sync::Semaphore;

const MAX_SESSION_BYTES: usize = 128 * 1024 * 1024;
const MAX_PROJECTS: usize = 16;

#[derive(Clone)]
pub struct App(Arc<Inner>);
struct Inner {
    catalog: cutting::Catalog,
    token: String,
    authority: String,
    projects: Mutex<BTreeMap<String, Arc<Vec<u8>>>>,
    work: Semaphore,
}
impl App {
    pub fn new(port: u16) -> Result<Self> {
        let mut random = [0_u8; 32];
        getrandom::fill(&mut random).map_err(|e| anyhow::anyhow!("session randomness: {e}"))?;
        Ok(Self(Arc::new(Inner {
            catalog: cutting::bundled()?,
            token: format!("{:x}", Sha256::digest(random)),
            authority: format!("127.0.0.1:{port}"),
            projects: Mutex::new(BTreeMap::new()),
            work: Semaphore::new(1),
        })))
    }
    fn token(&self, value: &str) -> Result<()> {
        ensure!(
            value == self.0.token,
            "form belongs to a different OpenWAM session; reopen the home page"
        );
        Ok(())
    }
    fn project(&self, id: &str) -> Result<Arc<Vec<u8>>> {
        self.0
            .projects
            .lock()
            .map_err(|_| anyhow::anyhow!("session store lock failed"))?
            .get(id)
            .cloned()
            .context("project not in this session; reopen your saved project")
    }
    fn retain(&self, bytes: Vec<u8>) -> Result<String> {
        ensure!(
            bytes.len() <= openwam::MAX_PROJECT_BYTES,
            "project exceeds 64 MiB"
        );
        let id = format!("{:x}", Sha256::digest(&bytes));
        let mut projects = self
            .0
            .projects
            .lock()
            .map_err(|_| anyhow::anyhow!("session store lock failed"))?;
        if !projects.contains_key(&id) {
            ensure!(
                projects.len() < MAX_PROJECTS,
                "session holds 16 project revisions; save your projects and restart OpenWAM"
            );
            let size: usize = projects.values().map(|v| v.len()).sum();
            ensure!(
                size + bytes.len() <= MAX_SESSION_BYTES,
                "session storage exceeds 128 MiB; save your projects and restart OpenWAM"
            );
            projects.insert(id.clone(), Arc::new(bytes));
        }
        Ok(id)
    }
}

struct UiError(anyhow::Error);
impl<E: Into<anyhow::Error>> From<E> for UiError {
    fn from(value: E) -> Self {
        Self(value.into())
    }
}
impl IntoResponse for UiError {
    fn into_response(self) -> Response {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Html(page(
                "Cannot prepare this project",
                &format!(
                    "<p class=\"notice\">{}</p><p><a href=\"/\">Return to OpenWAM</a></p>",
                    openwam::escape_html(&format!("{:#}", self.0))
                ),
            )),
        )
            .into_response()
    }
}
type UiResult = std::result::Result<Response, UiError>;

pub fn router(app: App) -> Router {
    Router::new()
        .route("/", get(home))
        .route("/prepare", post(prepare))
        .route("/reopen", post(reopen))
        .route("/projects/{id}", get(editor))
        .route("/projects/{id}/revise", post(revise))
        .route("/projects/{id}/download", get(download))
        .route("/projects/{id}/preview", get(preview))
        .route("/projects/{id}/simulate", post(simulate))
        .layer(DefaultBodyLimit::max(
            openwam::MAX_PROJECT_BYTES + 1024 * 1024,
        ))
        .layer(middleware::from_fn_with_state(app.clone(), guard))
        .with_state(app)
}

async fn guard(State(app): State<App>, request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok());
    let expected_origin = format!("http://{}", app.0.authority);
    let origin_valid = request
        .headers()
        .get(header::ORIGIN)
        .map(|v| v.to_str().ok() == Some(expected_origin.as_str()))
        .unwrap_or(true);
    if host != Some(app.0.authority.as_str()) || !origin_valid {
        return (
            StatusCode::FORBIDDEN,
            "OpenWAM accepts its own loopback origin only",
        )
            .into_response();
    }
    let Ok(_permit) = app.0.work.try_acquire() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "OpenWAM is preparing another request; retry shortly",
        )
            .into_response();
    };
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    headers.insert(header::CONTENT_SECURITY_POLICY, "default-src 'none'; style-src 'unsafe-inline'; frame-src 'self'; form-action 'self'; frame-ancestors 'self'; base-uri 'none'".parse().unwrap());
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
    response
}

fn page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>OpenWAM · {}</title><style>body{{font:16px system-ui;background:#101820;color:#edf4f7;margin:2rem}}main{{max-width:1100px;margin:auto}}a{{color:#66e1dc}}form{{padding:1.2rem;background:#182832;margin:1rem 0;border-radius:.5rem}}label{{display:block;margin:.8rem 0}}input,select,button{{font:inherit;padding:.5rem;max-width:100%;box-sizing:border-box}}select{{width:100%}}button{{background:#66e1dc;color:#101820;border:0;border-radius:.3rem;cursor:pointer}}.notice{{padding:1rem;border:1px solid #f2bc64}}iframe{{width:100%;height:900px;border:1px solid #637786}}code{{overflow-wrap:anywhere}}</style></head><body><main><a href=\"/\">OpenWAM</a><h1>{}</h1>{body}</main></body></html>",
        openwam::escape_html(title),
        openwam::escape_html(title)
    )
}

fn settings_form(
    app: &App,
    selected: Option<usize>,
    tolerance: f64,
    placement: [f64; 2],
) -> String {
    let mut html = format!(
        "<input type=\"hidden\" name=\"token\" value=\"{}\"><label>Machine · material · thickness · unit system<select name=\"row\" required>",
        app.0.token
    );
    for (index, row) in app.0.catalog.rows.iter().enumerate() {
        if row.cut.is_none() || !row.warnings.is_empty() {
            continue;
        }
        let model = match row.model {
            cutting::Model::Pro => "Pro",
            cutting::Model::Desktop => "Desktop",
        };
        let units = match row.units {
            cutting::Units::Metric => "mm",
            cutting::Units::Imperial => "in",
        };
        write!(
            html,
            "<option value=\"{index}\" {}>{model} · {} · {} {units}</option>",
            if selected == Some(index) {
                "selected"
            } else {
                ""
            },
            openwam::escape_html(&row.material),
            row.thickness
        )
        .expect("writing to String");
    }
    write!(html, "</select></label><label>Curve tolerance (mm)<input type=\"number\" name=\"tolerance\" min=\"0.000001\" max=\"1\" step=\"any\" value=\"{tolerance}\" required></label><label>Placement X (mm)<input type=\"number\" name=\"x\" step=\"any\" value=\"{}\" required></label><label>Placement Y down (mm)<input type=\"number\" name=\"y\" step=\"any\" value=\"{}\" required></label>", placement[0], placement[1]).expect("writing to String");
    html
}

async fn home(State(app): State<App>) -> Response {
    let selected = app.0.catalog.rows.iter().position(|r| {
        r.model == cutting::Model::Pro
            && r.units == cutting::Units::Metric
            && r.material == "Aluminum"
            && r.thickness == 3.0
    });
    Html(page("Prepare a local project", &format!(
        "<p>SVG/DXF → cutting settings → verified preview → saved project. Runs on this computer, without a vendor account.</p><p class=\"notice\">Draft planning only. Machine cut-file export is not yet available.</p><form action=\"/prepare\" method=\"post\" enctype=\"multipart/form-data\"><h2>Import a drawing</h2><label>SVG or planar DXF (up to 1 MiB)<input type=\"file\" name=\"drawing\" accept=\".svg,.dxf\" required></label>{}<button>Prepare and preview</button></form><form action=\"/reopen\" method=\"post\" enctype=\"multipart/form-data\"><h2>Reopen a saved project</h2><input type=\"hidden\" name=\"token\" value=\"{}\"><label>OpenWAM project<input type=\"file\" name=\"project\" accept=\".json\" required></label><button>Reopen and verify</button></form><p>Save a project before stopping the app. Unsaved revisions exist only in this session.</p>",
        settings_form(&app, selected, 0.02, [0.0, 0.0]), app.0.token
    ))).into_response()
}

struct Upload {
    data: Vec<u8>,
    filename: String,
    fields: BTreeMap<String, String>,
}
async fn upload(
    mut multipart: Multipart,
    file_field: &str,
    allowed: &[&str],
    limit: usize,
) -> Result<Upload> {
    let mut fields = BTreeMap::new();
    let mut file = None;
    while let Some(mut field) = multipart.next_field().await? {
        let name = field.name().context("unnamed form field")?.to_owned();
        ensure!(
            name == file_field || allowed.contains(&name.as_str()),
            "unexpected form field {name}"
        );
        let filename = field.file_name().unwrap_or("").to_owned();
        let bound = if name == file_field { limit } else { 1024 };
        let mut bytes = Vec::new();
        while let Some(chunk) = field.chunk().await? {
            ensure!(
                bytes.len() + chunk.len() <= bound,
                "form field {name} exceeds its size bound"
            );
            bytes.extend_from_slice(&chunk);
        }
        if name == file_field {
            ensure!(file.is_none(), "duplicate uploaded file");
            file = Some((filename, bytes));
        } else {
            ensure!(
                fields.insert(name, String::from_utf8(bytes)?).is_none(),
                "duplicate form field"
            );
        }
    }
    let (filename, data) = file.context("drawing/project file required")?;
    Ok(Upload {
        data,
        filename,
        fields,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    token: String,
    row: usize,
    tolerance: f64,
    x: f64,
    y: f64,
}
impl Settings {
    fn from_fields(fields: &BTreeMap<String, String>) -> Result<Self> {
        let value = |key| fields.get(key).with_context(|| format!("missing {key}"));
        Ok(Self {
            token: value("token")?.clone(),
            row: value("row")?.parse()?,
            tolerance: value("tolerance")?.parse()?,
            x: value("x")?.parse()?,
            y: value("y")?.parse()?,
        })
    }
}
fn prepare_bytes(
    app: &App,
    source: &[u8],
    format: bridge::SourceFormat,
    settings: &Settings,
) -> Result<Vec<u8>> {
    app.token(&settings.token)?;
    let row = app
        .0
        .catalog
        .rows
        .get(settings.row)
        .context("unknown material table point")?;
    let project = crate::openwam_api::prepare(
        source,
        format,
        &crate::openwam_api::Settings {
            model: row.model,
            material: row.material.clone(),
            units: row.units,
            thickness: row.thickness,
            tolerance_mm: settings.tolerance,
            placement_xy_down_mm: [settings.x, settings.y],
        },
        &app.0.catalog,
    )?;
    Ok(serde_json::to_vec_pretty(&project)?)
}
async fn prepare(State(app): State<App>, multipart: Multipart) -> UiResult {
    let upload = upload(
        multipart,
        "drawing",
        &["token", "row", "tolerance", "x", "y"],
        MAX_INPUT_BYTES,
    )
    .await?;
    let format = match std::path::Path::new(&upload.filename)
        .extension()
        .and_then(|v| v.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("svg") => bridge::SourceFormat::Svg,
        Some("dxf") => bridge::SourceFormat::Dxf,
        _ => return Err(anyhow::anyhow!("drawing filename must end in .svg or .dxf").into()),
    };
    let settings = Settings::from_fields(&upload.fields)?;
    let app_work = app.clone();
    let bytes = tokio::task::spawn_blocking(move || {
        prepare_bytes(&app_work, &upload.data, format, &settings)
    })
    .await??;
    let id = app.retain(bytes)?;
    Ok(Redirect::to(&format!("/projects/{id}")).into_response())
}
async fn reopen(State(app): State<App>, multipart: Multipart) -> UiResult {
    let upload = upload(multipart, "project", &["token"], openwam::MAX_PROJECT_BYTES).await?;
    app.token(
        upload
            .fields
            .get("token")
            .context("missing session token")?,
    )?;
    let app_work = app.clone();
    let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
        openwam::reopen(&upload.data, &app_work.0.catalog)?;
        Ok(upload.data)
    })
    .await??;
    let id = app.retain(bytes)?;
    Ok(Redirect::to(&format!("/projects/{id}")).into_response())
}
async fn editor(State(app): State<App>, Path(id): Path<String>) -> UiResult {
    let bytes = app.project(&id)?;
    let (project, report) = openwam::reopen(&bytes, &app.0.catalog)?;
    let row = &report.planning.selected_row;
    let selected = app.0.catalog.rows.iter().position(|r| {
        r.model == row.model
            && r.units == row.units
            && r.material == row.material
            && r.thickness == row.thickness
    });
    let html = format!(
        "<p><a href=\"/projects/{id}/download\">Save project</a> · <a href=\"/projects/{id}/preview\">Open full preview</a></p><p>Source format: {:?}. Reopened and independently verified.</p><form action=\"/projects/{id}/revise\" method=\"post\"><h2>Revise cutting settings and placement</h2>{}<button>Recompute revision</button></form><form action=\"/projects/{id}/simulate\" method=\"post\"><input type=\"hidden\" name=\"token\" value=\"{}\"><button>Download simulated replay</button></form><iframe title=\"Verified draft preview\" src=\"/projects/{id}/preview\"></iframe>",
        report.source_format,
        settings_form(
            &app,
            selected,
            report.transmog.requested_curve_tolerance_mm,
            report.transmog.placement_xy_down_mm
        ),
        app.0.token
    );
    drop(project);
    Ok(Html(page("Project preview and settings", &html)).into_response())
}
async fn revise(
    State(app): State<App>,
    Path(id): Path<String>,
    Form(settings): Form<Settings>,
) -> UiResult {
    app.token(&settings.token)?;
    let bytes = app.project(&id)?;
    let app_work = app.clone();
    let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
        let (project, report) = openwam::reopen(&bytes, &app_work.0.catalog)?;
        prepare_bytes(
            &app_work,
            project.source_text.as_bytes(),
            report.source_format,
            &settings,
        )
    })
    .await??;
    let id = app.retain(bytes)?;
    Ok(Redirect::to(&format!("/projects/{id}")).into_response())
}
async fn preview(State(app): State<App>, Path(id): Path<String>) -> UiResult {
    Ok(Html(openwam::preview(&app.project(&id)?, &app.0.catalog)?).into_response())
}
fn attachment(bytes: Vec<u8>, name: &'static str) -> Response {
    (
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CONTENT_DISPOSITION, name),
        ],
        bytes,
    )
        .into_response()
}
async fn download(State(app): State<App>, Path(id): Path<String>) -> UiResult {
    let bytes = app.project(&id)?;
    openwam::reopen(&bytes, &app.0.catalog)?;
    Ok(attachment(
        bytes.as_ref().clone(),
        "attachment; filename=project.openwam.json",
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Token {
    token: String,
}
async fn simulate(
    State(app): State<App>,
    Path(id): Path<String>,
    Form(token): Form<Token>,
) -> UiResult {
    app.token(&token.token)?;
    let bytes = app.project(&id)?;
    let result =
        tokio::task::spawn_blocking(move || openwam::simulate(&bytes, &app.0.catalog)).await??;
    Ok(attachment(
        serde_json::to_vec_pretty(&result)?,
        "attachment; filename=simulation.json",
    ))
}

#[cfg(test)]
mod tests;
