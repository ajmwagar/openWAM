use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use tower::ServiceExt;

const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="20mm" height="20mm" viewBox="0 0 20 20"><path id="line" d="M1 1L11 1"/></svg>"#;

fn row(app: &App) -> usize {
    app.0
        .catalog
        .rows
        .iter()
        .position(|r| {
            r.model == cutting::Model::Pro
                && r.units == cutting::Units::Metric
                && r.material == "Aluminum"
                && r.thickness == 3.0
        })
        .unwrap()
}
fn multipart(name: &str, filename: &str, bytes: &[u8], fields: &[(&str, String)]) -> Vec<u8> {
    let mut body = format!("--openwam-test\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n").into_bytes();
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n");
    for (key, value) in fields {
        body.extend_from_slice(format!("--openwam-test\r\nContent-Disposition: form-data; name=\"{key}\"\r\n\r\n{value}\r\n").as_bytes());
    }
    body.extend_from_slice(b"--openwam-test--\r\n");
    body
}
async fn request(
    app: &App,
    method: &str,
    path: &str,
    content_type: &str,
    body: Vec<u8>,
) -> Response {
    router(app.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header(header::HOST, &app.0.authority)
                .header(header::CONTENT_TYPE, content_type)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
}
async fn bytes(response: Response) -> Vec<u8> {
    to_bytes(
        response.into_body(),
        openwam::MAX_PROJECT_BYTES + 1024 * 1024,
    )
    .await
    .unwrap()
    .to_vec()
}
async fn create(app: &App) -> String {
    let body = multipart(
        "drawing",
        "drawing.svg",
        SVG,
        &[
            ("token", app.0.token.clone()),
            ("row", row(app).to_string()),
            ("tolerance", "0.02".into()),
            ("x", "0".into()),
            ("y", "0".into()),
        ],
    );
    let response = request(
        app,
        "POST",
        "/prepare",
        "multipart/form-data; boundary=openwam-test",
        body,
    )
    .await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    response.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn upload_preview_revision_save_and_simulate_use_the_same_owner_contract() {
    let app = App::new(8787).unwrap();
    let home = request(&app, "GET", "/", "", vec![]).await;
    assert_eq!(home.status(), StatusCode::OK);
    assert_eq!(home.headers()[header::CACHE_CONTROL], "no-store");
    let html = String::from_utf8(bytes(home).await).unwrap();
    assert!(html.contains("multipart/form-data") && !html.contains("<script"));
    let path = create(&app).await;
    let download = request(&app, "GET", &format!("{path}/download"), "", vec![]).await;
    assert!(
        download.headers()[header::CONTENT_DISPOSITION]
            .to_str()
            .unwrap()
            .contains("attachment")
    );
    let original = bytes(download).await;
    let (project, _) = openwam::reopen(&original, &app.0.catalog).unwrap();
    assert_eq!(project.source_text.as_bytes(), SVG);
    let preview = request(&app, "GET", &format!("{path}/preview"), "", vec![]).await;
    let html = String::from_utf8(bytes(preview).await).unwrap();
    assert!(html.contains("<svg") && html.contains("not a machine cut file"));
    let form = format!(
        "token={}&row={}&tolerance=0.02&x=2&y=3",
        app.0.token,
        row(&app)
    );
    let revised = request(
        &app,
        "POST",
        &format!("{path}/revise"),
        "application/x-www-form-urlencoded",
        form.into_bytes(),
    )
    .await;
    assert_eq!(revised.status(), StatusCode::SEE_OTHER);
    let revised_path = revised.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_owned();
    assert_ne!(path, revised_path);
    let revised =
        bytes(request(&app, "GET", &format!("{revised_path}/download"), "", vec![]).await).await;
    let (_, report) = openwam::reopen(&revised, &app.0.catalog).unwrap();
    assert_eq!(report.transmog.placement_xy_down_mm, [2.0, 3.0]);
    assert_eq!(
        bytes(request(&app, "GET", &format!("{path}/download"), "", vec![]).await).await,
        original
    );
    let editor =
        String::from_utf8(bytes(request(&app, "GET", &revised_path, "", vec![]).await).await)
            .unwrap();
    assert!(
        editor.contains("value=\"2\"")
            && editor.contains("value=\"3\"")
            && editor.contains("<iframe")
    );
    let simulation = request(
        &app,
        "POST",
        &format!("{revised_path}/simulate"),
        "application/x-www-form-urlencoded",
        format!("token={}", app.0.token).into_bytes(),
    )
    .await;
    assert_eq!(simulation.status(), StatusCode::OK);
    let simulation: serde_json::Value = serde_json::from_slice(&bytes(simulation).await).unwrap();
    assert_eq!(simulation["outcome"], "completed");
    assert_eq!(simulation["completed_operations"], 4);
    assert_eq!(simulation["machine_output_enabled"], false);
}

#[tokio::test]
async fn saved_projects_reopen_in_a_fresh_session_and_forged_operations_fail() {
    let app = App::new(8787).unwrap();
    let path = create(&app).await;
    let saved = bytes(request(&app, "GET", &format!("{path}/download"), "", vec![]).await).await;
    let fresh = App::new(8788).unwrap();
    let body = multipart(
        "project",
        "saved.json",
        &saved,
        &[("token", fresh.0.token.clone())],
    );
    let reopened = request(
        &fresh,
        "POST",
        "/reopen",
        "multipart/form-data; boundary=openwam-test",
        body,
    )
    .await;
    assert_eq!(reopened.status(), StatusCode::SEE_OTHER);
    let mut forged: serde_json::Value = serde_json::from_slice(&saved).unwrap();
    forged["integration"]["swarf"]["operations"][2]["to_mm"][0] = serde_json::json!(999.0);
    let body = multipart(
        "project",
        "forged.json",
        &serde_json::to_vec(&forged).unwrap(),
        &[("token", fresh.0.token.clone())],
    );
    let rejected = request(
        &fresh,
        "POST",
        "/reopen",
        "multipart/form-data; boundary=openwam-test",
        body,
    )
    .await;
    assert_eq!(rejected.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(fresh.0.projects.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn wrong_origin_host_session_and_duplicate_fields_cannot_change_projects() {
    let app = App::new(8787).unwrap();
    for (host, origin) in [
        ("evil.example", "http://evil.example"),
        ("127.0.0.1:8787", "http://evil.example"),
    ] {
        let response = router(app.clone())
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(header::HOST, host)
                    .header(header::ORIGIN, origin)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    for token_fields in [
        vec![("token", "wrong".into())],
        vec![
            ("token", app.0.token.clone()),
            ("token", app.0.token.clone()),
        ],
    ] {
        let mut fields = token_fields;
        fields.extend([
            ("row", row(&app).to_string()),
            ("tolerance", "0.02".into()),
            ("x", "0".into()),
            ("y", "0".into()),
        ]);
        let body = multipart("drawing", "drawing.svg", SVG, &fields);
        let response = request(
            &app,
            "POST",
            "/prepare",
            "multipart/form-data; boundary=openwam-test",
            body,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
    assert!(app.0.projects.lock().unwrap().is_empty());
}

#[tokio::test]
async fn oversized_uploads_invalid_settings_and_busy_requests_fail_loudly() {
    let app = App::new(8787).unwrap();
    let body = multipart("drawing", "huge.svg", &vec![b'x'; MAX_INPUT_BYTES + 1], &[]);
    let response = request(
        &app,
        "POST",
        "/prepare",
        "multipart/form-data; boundary=openwam-test",
        body,
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let settings = Settings {
        token: app.0.token.clone(),
        row: row(&app),
        tolerance: 0.0,
        x: 0.0,
        y: 0.0,
    };
    assert!(prepare_bytes(&app, SVG, bridge::SourceFormat::Svg, &settings).is_err());
    let _busy = app.0.work.acquire().await.unwrap();
    let response = request(&app, "GET", "/", "", vec![]).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(app.0.projects.lock().unwrap().is_empty());
}
