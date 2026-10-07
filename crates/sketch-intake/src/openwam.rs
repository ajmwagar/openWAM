//! Portable offline projects. Reopen through source/owner recomputation before
//! presenting operations; a preview is never a controller cut file.
use crate::{MAX_INPUT_BYTES, bridge, cutting::Catalog, simulation};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::fmt::Write;
use swarf_waterjet::Operation;

pub const MAX_PROJECT_BYTES: usize = 64 * 1024 * 1024;
const SCHEMA: &str = "reversal.openwam-project.v1";

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub schema: String,
    /// Exact UTF-8 source, including original whitespace and line endings.
    pub source_text: String,
    /// Owner report is the single source for settings, table binding and draft.
    pub integration: serde_json::Value,
}

pub fn create(original: &[u8], integration: &bridge::Integration) -> Result<Project> {
    ensure!(original.len() <= MAX_INPUT_BYTES, "source exceeds 1 MiB");
    let project = Project {
        schema: SCHEMA.into(),
        source_text: std::str::from_utf8(original)
            .context("project source must be UTF-8")?
            .into(),
        integration: serde_json::to_value(integration)?,
    };
    ensure!(
        serde_json::to_vec(&project)?.len() <= MAX_PROJECT_BYTES,
        "project exceeds 64 MiB"
    );
    Ok(project)
}

/// Original bytes are retained in the project; no filesystem references, hosted
/// credentials or cached operations are needed to independently rebuild it.
pub fn reopen(bytes: &[u8], catalog: &Catalog) -> Result<(Project, bridge::Integration)> {
    ensure!(bytes.len() <= MAX_PROJECT_BYTES, "project exceeds 64 MiB");
    let mut project: Project = serde_json::from_slice(bytes)?;
    ensure!(
        project.schema == SCHEMA,
        "unsupported OpenWAM project schema"
    );
    ensure!(
        project.source_text.len() <= MAX_INPUT_BYTES,
        "source exceeds 1 MiB"
    );
    let rebuilt = bridge::verified_integration(
        project.source_text.as_bytes(),
        &serde_json::to_vec(&project.integration)?,
        catalog,
    )?;
    // After exact semantic verification, return the owner's canonical numeric
    // representation so project hashes and downstream replay bindings are stable.
    project.integration = serde_json::to_value(&rebuilt)?;
    Ok((project, rebuilt))
}

pub fn check(bytes: &[u8], catalog: &Catalog) -> Result<serde_json::Value> {
    let (_, rebuilt) = reopen(bytes, catalog)?;
    Ok(serde_json::json!({
        "schema": "reversal.openwam-check.v1", "verified": true,
        "source_sha256": rebuilt.transmog.source_sha256,
        "operation_count": rebuilt.swarf.operations.len(),
        "machine_output_enabled": false,
        "blockers": rebuilt.swarf.blockers,
    }))
}

pub fn simulate(bytes: &[u8], catalog: &Catalog) -> Result<simulation::Replay> {
    let (project, _) = reopen(bytes, catalog)?;
    simulation::replay(
        project.source_text.as_bytes(),
        &serde_json::to_vec(&project.integration)?,
        catalog,
        simulation::FaultSpec {
            kind: simulation::FaultKind::None,
            at_ms: 0,
        },
    )
}

/// Escape text for quoted HTML attributes and text nodes.
pub fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Self-contained HTML/SVG derived only from verified numerical operations.
/// No source markup is embedded and no scripts or external assets are loaded.
pub fn preview_svg(bytes: &[u8], catalog: &Catalog) -> Result<String> {
    let (_, rebuilt) = reopen(bytes, catalog)?;
    draft_svg(&rebuilt.swarf)
}

/// Historical HTML report; product clients should consume SVG or typed operations.
pub fn preview(bytes: &[u8], catalog: &Catalog) -> Result<String> {
    let (_, rebuilt) = reopen(bytes, catalog)?;
    let draft = &rebuilt.swarf;
    let [width, depth] = draft.request.cutting_area_width_depth_mm;
    let mut html = String::from(
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'\"><title>OpenWAM · offline draft preview</title><style>body{font:16px system-ui;background:#101820;color:#edf4f7;margin:2rem}main{max-width:1100px;margin:auto}svg{width:100%;background:#182832;border:1px solid #637786}dt{color:#a7bdcc}dd{margin:0 0 1rem}code{overflow-wrap:anywhere}.notice{padding:1rem;border:1px solid #f2bc64}li{margin:.5rem 0}</style><main><h1>OpenWAM</h1><p>Offline draft preview</p><p class=\"notice\">Planning preview only. These paths have no kerf compensation, leads, tabs or qualified cutting order. This is not a machine cut file.</p><dl>",
    );
    write!(
        html,
        "<dt>Material</dt><dd>{} · {:.3} mm</dd><dt>Nominal cutting feed</dt><dd>{:.3} mm/min</dd><dt>Pierce time</dt><dd>{:.3} s</dd><dt>Nominal kerf (not applied)</dt><dd>{:.3} mm</dd><dt>Raw path length</dt><dd>{:.3} mm</dd><dt>Nominal cutting + piercing estimate</dt><dd>{:.1} s (excludes travel and hydraulic sequencing)</dd><dt>Source SHA-256</dt><dd><code>{}</code></dd></dl>",
        escape_html(&draft.request.material),
        draft.request.thickness_mm,
        draft.request.feed_mm_min,
        draft.request.pierce_seconds,
        draft.request.nominal_kerf_mm,
        draft.raw_centerline_length_mm,
        draft.nominal_cut_and_pierce_seconds,
        escape_html(&rebuilt.transmog.source_sha256)
    )?;
    write!(
        html,
        "<p>Bed: {width:.1} × {depth:.1} mm · cyan: cut · dashed grey: travel · amber: pierce. Top-left origin, Y down.</p>"
    )?;
    html.push_str(&draft_svg(draft)?);
    html.push_str("<h2>Remaining CAM work</h2><ul>");
    for blocker in &draft.blockers {
        write!(html, "<li>{}</li>", escape_html(blocker))?;
    }
    html.push_str("</ul><p>The project can be independently reopened and simulated with the local Rust CLI. No WAM account or network connection is required.</p></main></html>");
    Ok(html)
}

fn draft_svg(draft: &swarf_waterjet::Draft) -> Result<String> {
    let [width, depth] = draft.request.cutting_area_width_depth_mm;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}mm\" height=\"{depth}mm\" viewBox=\"0 0 {width} {depth}\" role=\"img\" aria-label=\"Verified waterjet draft paths\"><desc>Centreline draft preview: cyan cut, dashed grey travel, amber pierce. No kerf compensation or qualified cutting order. Not a machine cut file.</desc>"
    );
    let mut position = [0.0, 0.0];
    for operation in &draft.operations {
        match operation {
            Operation::Rapid { to_mm } | Operation::Cut { to_mm, .. } => {
                let travel = matches!(operation, Operation::Rapid { .. });
                write!(
                    svg,
                    "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"0.4\" {}/>",
                    position[0],
                    -position[1],
                    to_mm[0],
                    -to_mm[1],
                    if travel { "#8193a1" } else { "#66e1dc" },
                    if travel {
                        "stroke-dasharray=\"2 2\""
                    } else {
                        ""
                    }
                )?;
                position = *to_mm;
            }
            Operation::Pierce { .. } => {
                write!(
                    svg,
                    "<circle cx=\"{}\" cy=\"{}\" r=\"0.8\" fill=\"#f2bc64\"/>",
                    position[0], -position[1]
                )?;
            }
            Operation::Stop => (),
        }
    }
    svg.push_str("</svg>");
    Ok(svg)
}
