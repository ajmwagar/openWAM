//! Layout model: rotation, uniform scale, expanded copies and required stock.
//!
//! The single contour is deliberately not centrally symmetric, so a 180 degree
//! copy is a different polygon even though its bounding box is unchanged.
use reversal_sketch_intake::{
    bridge::{self, Layout, LayoutInstance},
    cutting,
    openwam_api::{self as api},
};
use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};

/// Local bounding box of the source contour: x 10..40, y -30..-10 (Y down).
const CONTOUR_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="60mm" viewBox="0 0 100 60"><path d="M10 10 H40 V20 H20 V30 H10 Z"/></svg>"#;
const CENTRE: [f64; 2] = [25.0, -20.0];
const LOCAL_BOX: [[f64; 2]; 2] = [[10.0, -30.0], [40.0, -10.0]];

fn invoke(request: &Value) -> (Value, Vec<u8>, i32) {
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
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (
        serde_json::from_slice(&output.stdout).unwrap(),
        output.stdout,
        output.status.code().unwrap(),
    )
}

fn envelope(operation: Value) -> Value {
    json!({"schema": api::REQUEST_SCHEMA, "request": operation})
}

fn instances(spec: &[(f64, f64, [f64; 2])]) -> Value {
    json!({"instances": spec.iter().map(|(rotation_deg, scale, placement_xy_down_mm)| json!({
        "rotation_deg": rotation_deg, "scale": scale, "placement_xy_down_mm": placement_xy_down_mm
    })).collect::<Vec<_>>()})
}

fn prepare_request(source: &str, layout: Option<Value>) -> Value {
    let mut request = api::capabilities()["prepare_example"].clone();
    request["request"]["source_text"] = json!(source);
    if let Some(layout) = layout {
        request["request"]["settings"]["layout"] = layout;
    }
    request
}

fn intent(ids: &[String]) -> Value {
    json!({"kerf_override_mm": null, "contours": ids.iter().map(|id| json!({
        "source_id": id, "side": "outside", "start_fraction": 0, "tabs": [],
        "lead_in_mm": 0, "lead_out_mm": 0
    })).collect::<Vec<_>>()})
}

/// Exact sine/cosine for the right-angle rotations used here, so the expected
/// stock below is arithmetic rather than a second trigonometry implementation.
fn right_angle(rotation_deg: f64) -> (f64, f64) {
    match rotation_deg as i64 {
        0 => (0.0, 1.0),
        90 => (1.0, 0.0),
        180 => (0.0, -1.0),
        270 => (-1.0, 0.0),
        other => panic!("not a right-angle rotation: {other}"),
    }
}

/// Union of the placed source bounding boxes, computed from the documented
/// placement semantics (`q = R * scale * (p - c) + c + placement`) over the four
/// local bounding-box corners.
fn expected_stock(spec: &[(f64, f64, [f64; 2])]) -> [[f64; 2]; 2] {
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for (rotation_deg, scale, [placement_x, placement_y]) in spec {
        let (sin, cos) = right_angle(*rotation_deg);
        for [x, y] in [
            [LOCAL_BOX[0][0], LOCAL_BOX[0][1]],
            [LOCAL_BOX[0][0], LOCAL_BOX[1][1]],
            [LOCAL_BOX[1][0], LOCAL_BOX[0][1]],
            [LOCAL_BOX[1][0], LOCAL_BOX[1][1]],
        ] {
            let dx = x - CENTRE[0];
            let dy = y - CENTRE[1];
            let placed_x = CENTRE[0] + scale * (cos * dx - sin * dy) + placement_x;
            let placed_y = CENTRE[1] + scale * (sin * dx + cos * dy) - placement_y;
            min = [min[0].min(placed_x), min[1].min(placed_y)];
            max = [max[0].max(placed_x), max[1].max(placed_y)];
        }
    }
    [min, max]
}

/// Analytic expectations cannot be exact: the importer round-trips through the
/// documented float32 normalized SVG frame (sub-micrometre error at plate size).
const ANALYTIC_TOLERANCE_MM: f64 = 1e-4;

fn assert_close(actual: &Value, expected: [f64; 2], what: &str) {
    let values = actual.as_array().unwrap_or_else(|| panic!("{what}"));
    assert_eq!(values.len(), 2, "{what}");
    for (actual, expected) in values.iter().zip(expected) {
        let actual = actual.as_f64().unwrap_or_else(|| panic!("{what}"));
        assert!(
            (actual - expected).abs() <= ANALYTIC_TOLERANCE_MM,
            "{what}: {actual} != {expected}"
        );
    }
}

/// Exact union of the expanded `transmog.features` contour points, read back
/// from the retained integration report rather than from the stock result.
fn placed_union(project: &Value) -> ([f64; 2], [f64; 2]) {
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for feature in project["integration"]["transmog"]["features"]
        .as_array()
        .unwrap()
    {
        for segment in feature["Sketch"]["profile"].as_array().unwrap() {
            for point in [&segment["Line"]["start"], &segment["Line"]["end"]] {
                let [x, y] = [point[0].as_f64().unwrap(), point[1].as_f64().unwrap()];
                min = [min[0].min(x), min[1].min(y)];
                max = [max[0].max(x), max[1].max(y)];
            }
        }
    }
    (min, max)
}

/// The stock is the union of the placed instance boxes, exactly as reported
/// geometry, and within importer precision of the documented placement math.
fn assert_stock(artifact: &Value, project: &Value, spec: &[(f64, f64, [f64; 2])]) {
    let (min, max) = placed_union(project);
    let stock = &artifact["stock"];
    assert_eq!(
        stock["min_xy_mm"],
        json!(min),
        "stock min is not the placed union"
    );
    assert_eq!(
        stock["max_xy_mm"],
        json!(max),
        "stock max is not the placed union"
    );
    assert_eq!(
        stock["width_depth_mm"],
        json!([max[0] - min[0], max[1] - min[1]]),
        "stock width/depth is not the placed union"
    );
    let [expected_min, expected_max] = expected_stock(spec);
    assert_close(&stock["min_xy_mm"], expected_min, "stock min_xy_mm");
    assert_close(&stock["max_xy_mm"], expected_max, "stock max_xy_mm");
    assert_close(
        &stock["width_depth_mm"],
        [
            expected_max[0] - expected_min[0],
            expected_max[1] - expected_min[1],
        ],
        "stock width_depth_mm",
    );
}

fn grid(count: usize, columns: usize, step: [f64; 2], start: [f64; 2]) -> Vec<[f64; 2]> {
    (0..count)
        .map(|index| {
            let (column, row) = (index % columns, index / columns);
            [
                start[0] + step[0] * column as f64,
                start[1] + step[1] * row as f64,
            ]
        })
        .collect()
}

fn expanded_ids(count: usize) -> Vec<String> {
    (0..count)
        .map(|index| format!("svg-contour-0#{index}"))
        .collect()
}

#[test]
fn ten_instance_layout_with_mixed_rotation_and_uniform_scale_prepares_plans_and_verifies() {
    let spec: Vec<(f64, f64, [f64; 2])> = grid(10, 5, [60.0, 40.0], [20.0, 20.0])
        .into_iter()
        .enumerate()
        .map(|(index, placement)| {
            let rotation_deg = if index % 2 == 0 { 0.0 } else { 180.0 };
            (rotation_deg, 0.5, placement)
        })
        .collect();
    let (prepared, _, prepare_exit) = invoke(&prepare_request(CONTOUR_SVG, Some(instances(&spec))));
    assert_eq!(prepare_exit, 0, "{prepared}");
    assert_eq!(prepared["machine_output_enabled"], false);
    let project = prepared["result"]["project"].clone();
    let transmog = &project["integration"]["transmog"];
    // The layout is retained so reopen can rebuild the same copies.
    assert_eq!(
        transmog["layout"]["instances"].as_array().unwrap().len(),
        10
    );
    // Expanded identities are stable, instance-major, per-contour order preserved.
    let identity: Vec<&str> = transmog["contour_identity"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["source_id"].as_str().unwrap())
        .collect();
    assert_eq!(
        identity,
        expanded_ids(10)
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
    );
    assert_eq!(transmog["features"].as_array().unwrap().len(), 10);

    let ids = expanded_ids(10);
    let (planned, _, plan_exit) = invoke(&envelope(
        json!({"operation": "plan_cam", "project": project, "intent": intent(&ids)}),
    ));
    assert_eq!(plan_exit, 0, "{planned}");
    let artifact = planned["result"]["artifact"].clone();
    assert_eq!(artifact["machine_output_enabled"], false);
    assert_eq!(artifact["plan"]["paths"].as_array().unwrap().len(), 10);
    // One placed copy per expanded contour, in retained order.
    assert_eq!(artifact["plan"]["paths"][5]["source_id"], "svg-contour-0#5");
    assert_stock(&artifact, &project, &spec);

    let (verified, _, verify_exit) = invoke(&envelope(
        json!({"operation": "verify_cam", "artifact": artifact}),
    ));
    assert_eq!(verify_exit, 0, "{verified}");
    assert_eq!(verified["result"]["verification"]["verified"], true);
    assert_eq!(verified["machine_output_enabled"], false);

    // Stock is part of the independent rebuild, not a decoration.
    let mut forged = artifact.clone();
    forged["stock"]["max_xy_mm"][0] =
        json!(artifact["stock"]["max_xy_mm"][0].as_f64().unwrap() + 1.0);
    let (rejected, _, exit) = invoke(&envelope(
        json!({"operation": "verify_cam", "artifact": forged}),
    ));
    assert_ne!(exit, 0);
    assert_eq!(rejected["error"]["code"], "cam_invalid");
    let mut dropped = artifact.clone();
    dropped["stock"] = Value::Null;
    let (rejected, _, exit) = invoke(&envelope(
        json!({"operation": "verify_cam", "artifact": dropped}),
    ));
    assert_ne!(exit, 0, "{rejected}");
    assert_eq!(rejected["error"]["code"], "cam_invalid");

    // A retained source plus typed intent still rebuilds the whole layout.
    let (verified, _, exit) = invoke(&envelope(
        json!({"operation": "verify", "project": project}),
    ));
    assert_eq!(exit, 0, "{verified}");
    assert_eq!(verified["result"]["verification"]["verified"], true);
}

#[test]
fn stock_size_is_the_union_of_placed_instance_bounding_boxes() {
    // Mixed rotations and non-uniform per-instance scales: the stock must follow
    // each instance's own transform, not one global placement.
    let spec: Vec<(f64, f64, [f64; 2])> = vec![
        (0.0, 1.0, [40.0, 10.0]),
        (90.0, 0.5, [140.0, 15.0]),
        (180.0, 0.75, [240.0, 20.0]),
    ];
    let (prepared, _, exit) = invoke(&prepare_request(CONTOUR_SVG, Some(instances(&spec))));
    assert_eq!(exit, 0, "{prepared}");
    let project = prepared["result"]["project"].clone();
    let ids = expanded_ids(spec.len());
    let (planned, _, exit) = invoke(&envelope(
        json!({"operation": "plan_cam", "project": project, "intent": intent(&ids)}),
    ));
    assert_eq!(exit, 0, "{planned}");
    let artifact = planned["result"]["artifact"].clone();
    assert_stock(&artifact, &project, &spec);
    let stock = &artifact["stock"];
    // Union of [50,80]x[-40,-20], [160,170]x[-42.5,-27.5], [253.75,276.25]x[-47.5,-32.5].
    assert_close(&stock["min_xy_mm"], [50.0, -47.5], "stock min_xy_mm");
    assert_close(&stock["max_xy_mm"], [276.25, -20.0], "stock max_xy_mm");
    assert_close(
        &stock["width_depth_mm"],
        [226.25, 27.5],
        "stock width_depth_mm",
    );
    let (verified, _, exit) = invoke(&envelope(
        json!({"operation": "verify_cam", "artifact": artifact}),
    ));
    assert_eq!(exit, 0, "{verified}");
    assert_eq!(verified["result"]["verification"]["verified"], true);
}

#[test]
fn over_bed_layout_is_rejected_with_the_owner_cutting_area_error() {
    // Local x reaches 40 mm, so a 450 mm placement crosses the 460 mm bed edge.
    let (response, _, exit) = invoke(&prepare_request(
        CONTOUR_SVG,
        Some(instances(&[(0.0, 1.0, [450.0, 20.0])])),
    ));
    assert_eq!(exit, 2, "{response}");
    assert_eq!(response["status"], "error");
    assert_eq!(response["error"]["code"], "planning_failed");
    let message = response["error"]["message"].as_str().unwrap();
    assert!(
        message.contains("cutting area"),
        "message does not name the cutting-area breach: {message}"
    );
    assert_eq!(response["machine_output_enabled"], false);

    // The same layout is rejected before any CAM artifact exists.
    let (response, _, exit) = invoke(&prepare_request(
        CONTOUR_SVG,
        Some(instances(&[(0.0, 1.0, [0.0, 290.0])])),
    ));
    assert_eq!(exit, 2, "{response}");
    assert!(
        response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("cutting area")
    );
}

#[test]
fn single_instance_output_is_byte_identical_to_the_retained_base_fixtures() {
    // (d) prepare: exact CLI response bytes for examples/plate.svg.
    let (_, bytes, exit) = invoke(&api::capabilities()["prepare_example"]);
    assert_eq!(exit, 0);
    assert_eq!(
        bytes,
        include_bytes!("../../../wazer/fixtures/plate-prepare.response.json"),
        "single-instance prepare response is not byte-identical to the base commit"
    );

    // (d) plan_cam: exact artifact bytes for wazer/fixtures/cam-panel.svg.
    let (prepared, _, exit) = invoke(&prepare_request(
        include_str!("../../../wazer/fixtures/cam-panel.svg"),
        None,
    ));
    assert_eq!(exit, 0, "{prepared}");
    let intent: Value = serde_json::from_str(include_str!(
        "../../../wazer/fixtures/cam-panel.intent.json"
    ))
    .unwrap();
    let (planned, _, exit) = invoke(&envelope(json!({
        "operation": "plan_cam", "project": prepared["result"]["project"], "intent": intent
    })));
    assert_eq!(exit, 0, "{planned}");
    let artifact = &planned["result"]["artifact"];
    // A project without a layout reports no stock at all.
    assert!(artifact.get("stock").is_none(), "{artifact}");
    let fixture = include_str!("../../../wazer/fixtures/cam-panel-plan.artifact.json");
    assert_eq!(
        format!("{}\n", serde_json::to_string(artifact).unwrap()),
        fixture,
        "single-instance CAM artifact is not byte-identical to the base commit"
    );
}

fn settings_with(layout: Option<Layout>, placement_xy_down_mm: [f64; 2]) -> api::Settings {
    api::Settings {
        model: cutting::Model::Pro,
        material: "Aluminum".into(),
        units: cutting::Units::Metric,
        thickness: 3.0,
        tolerance_mm: 0.02,
        placement_xy_down_mm,
        layout,
    }
}

fn instance(rotation_deg: f64, scale: f64, placement: [f64; 2]) -> LayoutInstance {
    LayoutInstance {
        rotation_deg,
        scale,
        placement_xy_down_mm: placement,
    }
}

fn rejected(layout: Option<Layout>, placement: [f64; 2]) -> String {
    let catalog = cutting::bundled().unwrap();
    let error = api::prepare(
        CONTOUR_SVG.as_bytes(),
        bridge::SourceFormat::Svg,
        &settings_with(layout, placement),
        &catalog,
    )
    .expect_err("layout must be rejected, never repaired");
    format!("{error:#}")
}

#[test]
fn non_finite_mirrored_empty_and_ambiguous_layouts_are_rejected_explicitly() {
    for (layout, placement, expected) in [
        (
            Some(Layout {
                instances: Vec::new(),
            }),
            [0.0, 0.0],
            "at least one instance",
        ),
        (
            Some(Layout {
                instances: vec![instance(0.0, 1.0, [10.0, 10.0]); bridge::MAX_INSTANCES + 1],
            }),
            [0.0, 0.0],
            "exceeds 64 instances",
        ),
        (
            Some(Layout {
                instances: vec![instance(f64::NAN, 1.0, [10.0, 10.0])],
            }),
            [0.0, 0.0],
            "rotation must be finite",
        ),
        (
            Some(Layout {
                instances: vec![instance(f64::INFINITY, 1.0, [10.0, 10.0])],
            }),
            [0.0, 0.0],
            "rotation must be finite",
        ),
        (
            Some(Layout {
                instances: vec![instance(0.0, 0.0, [10.0, 10.0])],
            }),
            [0.0, 0.0],
            "greater than zero; mirroring is unsupported",
        ),
        (
            Some(Layout {
                instances: vec![instance(0.0, -1.0, [10.0, 10.0])],
            }),
            [0.0, 0.0],
            "greater than zero; mirroring is unsupported",
        ),
        (
            Some(Layout {
                instances: vec![instance(0.0, f64::NAN, [10.0, 10.0])],
            }),
            [0.0, 0.0],
            "greater than zero; mirroring is unsupported",
        ),
        (
            Some(Layout {
                instances: vec![instance(0.0, 1.0, [f64::NAN, 10.0])],
            }),
            [0.0, 0.0],
            "placement must be finite",
        ),
        (
            Some(Layout {
                instances: vec![instance(0.0, 1.0, [10.0, 10.0])],
            }),
            [1.0, 0.0],
            "per-instance placement is authoritative",
        ),
    ] {
        let message = rejected(layout, placement);
        assert!(
            message.contains(expected),
            "{message} does not contain {expected}"
        );
    }
    // A layout copy that leaves the finite range is rejected, not clamped.
    let message = rejected(
        Some(Layout {
            instances: vec![instance(0.0, 1.0, [f64::MAX, 0.0])],
        }),
        [0.0, 0.0],
    );
    assert!(message.contains("cutting area"), "{message}");
}

#[test]
fn expanded_operation_budget_is_the_owner_bound_not_a_new_limit() {
    // Three source contours times 64 instances needs 192 CAM contours, above the
    // owner's unchanged 128-contour intent bound.
    let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40mm" height="40mm" viewBox="0 0 40 40"><path d="M1 1H3V3H1Z"/><path d="M5 1H7V3H5Z"/><path d="M1 5H3V7H1Z"/></svg>"#;
    let spec: Vec<(f64, f64, [f64; 2])> =
        grid(bridge::MAX_INSTANCES, 8, [10.0, 10.0], [10.0, 10.0])
            .into_iter()
            .map(|placement| (0.0, 1.0, placement))
            .collect();
    let (prepared, _, exit) = invoke(&prepare_request(source, Some(instances(&spec))));
    assert_eq!(exit, 0, "{prepared}");
    let project = prepared["result"]["project"].clone();
    let ids: Vec<String> = (0..bridge::MAX_INSTANCES)
        .flat_map(|index| (0..3).map(move |contour| format!("svg-contour-{contour}#{index}")))
        .collect();
    assert_eq!(ids.len(), 192);
    let (response, _, exit) = invoke(&envelope(json!({
        "operation": "plan_cam", "project": project, "intent": intent(&ids)
    })));
    assert_eq!(exit, 2, "{response}");
    assert_eq!(response["error"]["code"], "cam_invalid");
    assert!(
        response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("max 128 contours"),
        "{response}"
    );
}
