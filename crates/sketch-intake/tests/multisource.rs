//! Multi-source bundles and user-defined materials through the public request API.
use reversal_sketch_intake::openwam_api as api;
use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};

const SQUARE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40mm" height="40mm" viewBox="0 0 40 40"><path id="sq" d="M5 5 H35 V35 H5 Z"/></svg>"#;
const PLATE: &str = include_str!("../examples/plate.svg");

fn invoke(request: &Value) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_openwam"))
        .arg("request")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(request).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    serde_json::from_slice(&output.stdout).unwrap()
}

fn envelope(operation: Value) -> Value {
    json!({"schema": api::REQUEST_SCHEMA, "request": operation})
}

fn settings() -> Value {
    json!({"model": "pro", "material": "Aluminum", "units": "metric", "thickness": 3.0,
        "tolerance_mm": 0.02, "placement_xy_down_mm": [0.0, 0.0]})
}

fn source(id: &str, name: &str, text: &str, placement: [f64; 2]) -> Value {
    json!({"id": id, "name": name, "format": "svg", "text": text, "placement_xy_down_mm": placement})
}

fn prepare(sources: Value, settings: Value) -> Value {
    invoke(&envelope(
        json!({"operation": "prepare_sources", "sources": sources, "settings": settings}),
    ))
}

fn project(response: &Value) -> &Value {
    assert_eq!(response["status"], "ok", "{response}");
    assert_eq!(response["machine_output_enabled"], false);
    &response["result"]["project"]
}

fn intent_for(project: &Value) -> Value {
    let contours: Vec<Value> = project["integration"]["transmog"]["contour_identity"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            json!({"source_id": c["source_id"],
                "side": if c["closed"] != true { "no_cut" }
                    else if c["source_id"].as_str().unwrap().ends_with(":hole") { "inside" }
                    else { "outside" },
                "start_fraction": 0, "tabs": [], "lead_in_mm": 0, "lead_out_mm": 0})
        })
        .collect();
    json!({"kerf_override_mm": null, "contours": contours})
}

fn plan(project: &Value) -> Value {
    invoke(&envelope(
        json!({"operation": "plan_cam", "project": project, "intent": intent_for(project)}),
    ))
}

fn verify(artifact: &Value) -> Value {
    invoke(&envelope(
        json!({"operation": "verify_cam", "artifact": artifact}),
    ))
}

#[test]
fn two_sources_with_copies_plan_verify_and_report_per_source_geometry() {
    let mut second = source("b", "square.svg", SQUARE, [0.0, 0.0]);
    second["layout"] = json!({"instances": [
        {"rotation_deg": 0, "scale": 1, "placement_xy_down_mm": [120, 10]},
        {"rotation_deg": 0, "scale": 1, "placement_xy_down_mm": [200, 10]}]});
    let response = prepare(
        json!([source("a", "plate.svg", PLATE, [0.0, 0.0]), second]),
        settings(),
    );
    let project = project(&response);
    let transmog = &project["integration"]["transmog"];
    assert_eq!(transmog["layout"], Value::Null);
    let sources = transmog["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0]["name"], "plate.svg");
    assert_eq!(sources[1]["instances"].as_array().unwrap().len(), 2);
    let second_box = &sources[1]["instances"][1];
    // Square spans 5..35 in the source; the second copy shifts by 200 right and 10 down.
    for (key, want) in [
        ("bbox_min_xy_down_mm", [205.0, 15.0]),
        ("bbox_max_xy_down_mm", [235.0, 45.0]),
    ] {
        for axis in 0..2 {
            let got = second_box[key][axis].as_f64().unwrap();
            assert!((got - want[axis]).abs() < 1e-3, "{key}[{axis}] = {got}");
        }
    }
    let ids = sources[1]["contour_ids"].as_array().unwrap();
    assert_eq!(ids, &[json!("b:sq#0"), json!("b:sq#1")]);
    // 3 plate contours + 2 square copies.
    assert_eq!(transmog["features"].as_array().unwrap().len(), 5);

    let planned = plan(project);
    assert_eq!(planned["status"], "ok", "{planned}");
    let artifact = &planned["result"]["artifact"];
    assert_eq!(artifact["machine_output_enabled"], false);
    assert!(
        artifact["stock"].is_object(),
        "bundle reports required stock"
    );
    let verified = verify(artifact);
    assert_eq!(
        verified["result"]["verification"]["verified"], true,
        "{verified}"
    );

    // A moved source changes the project: the retained artifact no longer verifies.
    let mut moved = artifact.clone();
    let text = moved["project"]["source_text"]
        .as_str()
        .unwrap()
        .replace("[200.0,10.0]", "[210.0,10.0]");
    assert_ne!(text, moved["project"]["source_text"].as_str().unwrap());
    moved["project"]["source_text"] = json!(text);
    assert_eq!(verify(&moved)["status"], "error");
}

#[test]
fn bundle_rejects_bad_ids_names_and_duplicates() {
    for bad in [
        json!([source("a/b", "x.svg", SQUARE, [0.0, 0.0])]),
        json!([source("a", "../x.svg", SQUARE, [0.0, 0.0])]),
        json!([source("a", "d\\x.svg", SQUARE, [0.0, 0.0])]),
        json!([
            source("a", "x.svg", SQUARE, [0.0, 0.0]),
            source("a", "y.svg", SQUARE, [0.0, 0.0])
        ]),
        json!([]),
    ] {
        assert_eq!(prepare(bad, settings())["status"], "error");
    }
    let mut missing = source("a", "x.svg", SQUARE, [0.0, 0.0]);
    missing["contours"] = json!(["nope"]);
    assert_eq!(prepare(json!([missing]), settings())["status"], "error");
}

#[test]
fn contour_selection_keeps_only_that_cut_group() {
    let mut part = source("a", "plate.svg", PLATE, [0.0, 0.0]);
    part["contours"] = json!(["outline", "hole"]);
    let response = prepare(json!([part]), settings());
    let ids = project(&response)["integration"]["transmog"]["sources"][0]["contour_ids"].clone();
    assert_eq!(ids, json!(["a:outline", "a:hole"]));
}

fn recipe() -> Value {
    json!({"user_defined": true, "name": "Polyurethane", "category": "Plastics & Rubbers",
        "model": "pro", "units": "imperial", "thickness": 0.5, "feed": 23.3, "pierce_seconds": 4.0,
        "lead_length": 0.15, "tab_size": 0.0, "cut_rate": "fine", "origin": "test"})
}

fn user_settings() -> Value {
    json!({"model": "pro", "material": "Polyurethane", "units": "imperial", "thickness": 0.5,
        "tolerance_mm": 0.02, "placement_xy_down_mm": [0.0, 0.0], "user_material": recipe()})
}

#[test]
fn user_material_is_marked_bound_and_never_enters_the_catalog() {
    let response = prepare(
        json!([source("a", "sq.svg", SQUARE, [0.0, 0.0])]),
        user_settings(),
    );
    let project = project(&response);
    let planning = &project["integration"]["planning"];
    assert_eq!(planning["selected_row"]["user_defined"], true);
    assert_eq!(planning["user_material"]["feed"], 23.3);
    assert!((planning["feed_mm_min"].as_f64().unwrap() - 23.3 * 25.4).abs() < 1e-9);

    let catalog = invoke(&envelope(json!({"operation": "catalog"})));
    let rows = catalog["result"]["rows"].as_array().unwrap();
    assert!(rows.iter().all(|r| r.get("user_defined").is_none()));
    assert!(rows.iter().all(|r| r["material"] != "Polyurethane"
        || r["thickness"] != 0.5
        || r["cut"]["feed"] != 23.3));

    let planned = plan(project);
    let artifact = planned["result"]["artifact"].clone();
    assert_eq!(
        verify(&artifact)["result"]["verification"]["verified"],
        true
    );

    // A changed recipe changes the plan and the project hash: the artifact is rejected.
    let mut tampered = artifact.clone();
    tampered["project"]["integration"]["planning"]["user_material"]["feed"] = json!(30.0);
    assert_eq!(verify(&tampered)["status"], "error");
}

#[test]
fn user_material_must_be_explicit_and_consistent() {
    let sources = json!([source("a", "sq.svg", SQUARE, [0.0, 0.0])]);
    for edit in [
        |s: &mut Value| s["user_material"]["user_defined"] = json!(false),
        |s: &mut Value| s["user_material"]["feed"] = json!(0),
        |s: &mut Value| s["user_material"]["name"] = json!("Other"),
        |s: &mut Value| s["thickness"] = json!(0.75),
        |s: &mut Value| s["user_material"]["pierce_seconds"] = json!(-1),
    ] {
        let mut s = user_settings();
        edit(&mut s);
        assert_eq!(prepare(sources.clone(), s)["status"], "error");
    }
}

#[test]
fn wam_extras_must_agree_with_the_selected_feed() {
    let sources = json!([source("a", "sq.svg", SQUARE, [0.0, 0.0])]);
    let wam = |fine: f64| {
        json!({"uid": 257, "speed_rates": [29.1, 26.2, fine], "eco_rates": [2.756, 2.48, 2.205],
            "abrasive_g_min": 150, "max_tab": 3})
    };
    let mut ok = user_settings();
    ok["user_material"]["wam"] = wam(23.3);
    assert_eq!(prepare(sources.clone(), ok)["status"], "ok");
    let mut bad = user_settings();
    bad["user_material"]["wam"] = wam(20.0);
    assert_eq!(prepare(sources, bad)["status"], "error");
}

#[test]
fn single_source_prepare_is_unchanged_and_has_no_bundle_fields() {
    let response = invoke(&api::capabilities()["prepare_example"]);
    let planning = &project(&response)["integration"]["planning"];
    assert!(planning.get("user_material").is_none());
    assert!(planning["selected_row"].get("user_defined").is_none());
    assert!(
        project(&response)["integration"]["transmog"]
            .get("sources")
            .is_none()
    );
}

fn stroked(paths: &str) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="60mm" height="60mm" viewBox="0 0 60 60">{paths}</svg>"#
    )
}

#[test]
fn gap_rules_close_merge_and_drop_like_wam_import() {
    // Square whose ends miss by 0.01 mm: closed, so it plans as a cut contour.
    let near = stroked(r#"<path id="sq" fill="none" stroke="black" d="M5 5 H35 V35 H5 V5.01"/>"#);
    let r = prepare(
        json!([source("a", "near.svg", &near, [0.0, 0.0])]),
        settings(),
    );
    let p = project(&r);
    let src = &p["integration"]["transmog"]["sources"][0];
    assert_eq!(
        p["integration"]["transmog"]["contour_identity"][0]["closed"],
        true
    );
    assert!(
        src["normalized"][0]
            .as_str()
            .unwrap()
            .starts_with("closed sq")
    );
    assert_eq!(plan(p)["status"], "ok");

    // Two halves 0.02 mm apart are merged, then closed.
    let halves = stroked(
        r#"<path id="a" fill="none" stroke="black" d="M5 5 H35 V20"/><path id="b" fill="none" stroke="black" d="M35 20.02 V35 H5 V5.02"/>"#,
    );
    let r = prepare(
        json!([source("a", "halves.svg", &halves, [0.0, 0.0])]),
        settings(),
    );
    let p = project(&r);
    let t = &p["integration"]["transmog"];
    assert_eq!(t["contour_identity"].as_array().unwrap().len(), 1);
    assert_eq!(t["contour_identity"][0]["closed"], true);
    assert_eq!(t["sources"][0]["normalized"].as_array().unwrap().len(), 2);
    assert_eq!(plan(p)["status"], "ok");

    // A 0.5 mm gap stays open (no-cut by default); a 0.001 mm stub is dropped.
    let wide = stroked(
        r#"<path id="w" fill="none" stroke="black" d="M5 5 H35 V35 H5 V4.5"/><path id="t" fill="none" stroke="black" d="M50 50 L50.001 50"/>"#,
    );
    let r = prepare(
        json!([source("a", "wide.svg", &wide, [0.0, 0.0])]),
        settings(),
    );
    let p = project(&r);
    let ids = p["integration"]["transmog"]["contour_identity"]
        .as_array()
        .unwrap();
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0]["closed"], false);
    assert!(
        p["integration"]["transmog"]["sources"][0]["normalized"][0]
            .as_str()
            .unwrap()
            .starts_with("dropped t")
    );
}
