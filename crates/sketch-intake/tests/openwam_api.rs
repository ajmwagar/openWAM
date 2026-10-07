use reversal_sketch_intake::openwam_api::{self as api, ErrorCode};
use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn invoke(request: &Value) -> (Value, bool) {
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
        output.status.success(),
    )
}
fn envelope(operation: Value) -> Value {
    json!({"schema":api::REQUEST_SCHEMA,"request":operation})
}

#[test]
fn agent_cli_prepare_revise_verify_and_fault_replay_are_stateless_and_recomputable() {
    let example = api::capabilities()["prepare_example"].clone();
    let (prepared, success) = invoke(&example);
    assert!(success);
    let project = prepared["result"]["project"].clone();
    assert_eq!(prepared["machine_output_enabled"], false);
    let (verified, success) = invoke(&envelope(json!({"operation":"verify","project":project})));
    assert!(success && verified["result"]["verification"]["verified"] == true);
    let mut settings = example["request"]["settings"].clone();
    settings["placement_xy_down_mm"] = json!([2.0, 3.0]);
    let (revision, success) = invoke(&envelope(
        json!({"operation":"revise","project":project,"settings":settings}),
    ));
    assert!(success);
    assert_eq!(
        revision["result"]["project"]["integration"]["transmog"]["placement_xy_down_mm"],
        json!([2.0, 3.0])
    );
    assert_eq!(
        project["integration"]["transmog"]["placement_xy_down_mm"],
        json!([0.0, 0.0])
    );
    let (replayed, success) = invoke(&envelope(
        json!({"operation":"simulate","project":project,"fault":{"kind":"enclosure_open","at_ms":10000}}),
    ));
    assert!(success);
    let replay = replayed["result"]["replay"].clone();
    assert_eq!(replay["outcome"], "faulted");
    assert_eq!(replay["virtual_elapsed_ms"], 10000);
    let (verified, success) = invoke(&envelope(
        json!({"operation":"verify_simulation","project":project,"replay":replay}),
    ));
    assert!(success && verified["result"]["verification"]["verified"] == true);
    let mut forged = replay;
    forged["frames"][0]["pressure_units"] = json!(999);
    let (rejected, success) = invoke(&envelope(
        json!({"operation":"verify_simulation","project":project,"replay":forged}),
    ));
    assert!(!success);
    assert_eq!(rejected["error"]["code"], "invalid_replay");
}

#[test]
fn catalog_preview_and_project_tampering_use_structured_outputs() {
    let (catalog, success) = invoke(&envelope(
        json!({"operation":"catalog","model":"pro","material":"Aluminum","units":"metric"}),
    ));
    assert!(success);
    assert!(
        catalog["result"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["model"] == "pro" && r["material"] == "Aluminum" && r["units"] == "metric")
    );
    let (prepared, success) = invoke(&api::capabilities()["prepare_example"]);
    assert!(success);
    assert_eq!(
        catalog["result"]["tables_canonical_sha256"],
        prepared["result"]["project"]["integration"]["planning"]["tables_canonical_sha256"]
    );
    assert!(catalog["result"]["materials_source"]["url"].is_string());
    let mut project = prepared["result"]["project"].clone();
    let (preview, success) = invoke(&envelope(json!({"operation":"preview","project":project})));
    assert!(success);
    assert_eq!(preview["result"]["media_type"], "image/svg+xml");
    let svg = preview["result"]["content"].as_str().unwrap();
    let doc = roxmltree::Document::parse(svg).unwrap();
    assert_eq!(doc.root_element().tag_name().name(), "svg");
    assert!(
        doc.descendants()
            .all(|n| !matches!(n.tag_name().name(), "script" | "style" | "html"))
    );
    assert!(
        preview["result"]["content"]
            .as_str()
            .unwrap()
            .contains("<svg")
    );
    project["integration"]["machine_output_enabled"] = json!(true);
    let (rejected, success) = invoke(&envelope(json!({"operation":"verify","project":project})));
    assert!(!success);
    assert_eq!(rejected["error"]["code"], "invalid_project");
}

#[test]
fn malformed_unknown_fields_versions_limits_and_cli_io_have_stable_errors() {
    for (input, code) in [
        (b"not-json".as_slice(), ErrorCode::InvalidRequest),
        (br#"{"schema":"other","request":{"operation":"catalog"}}"#.as_slice(), ErrorCode::UnsupportedSchema),
        (br#"{"schema":"reversal.openwam-request.v1","request":{"operation":"catalog","machine_output_enabled":true}}"#.as_slice(), ErrorCode::InvalidRequest),
    ] {
        let response = serde_json::to_value(api::dispatch(input)).unwrap();
        assert_eq!(response["error"]["code"], serde_json::to_value(code).unwrap());
    }
    let mut too_large = api::capabilities()["prepare_example"].clone();
    too_large["request"]["source_text"] =
        json!("x".repeat(reversal_sketch_intake::MAX_INPUT_BYTES + 1));
    let (response, success) = invoke(&too_large);
    assert!(!success);
    assert_eq!(response["error"]["code"], "input_too_large");
    for (args, code) in [
        (vec!["unknown"], "usage"),
        (
            vec!["request", "/nonexistent/openwam-request.json"],
            "io_failed",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_openwam"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stderr.is_empty());
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap()["error"]["code"],
            code
        );
    }
}

#[test]
fn capabilities_generate_request_and_response_schemas_and_file_input_works() {
    let output = Command::new(env!("CARGO_BIN_EXE_openwam"))
        .arg("capabilities")
        .output()
        .unwrap();
    assert!(output.status.success() && output.stderr.is_empty());
    let capabilities: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(capabilities["request_version"], api::REQUEST_SCHEMA);
    assert!(capabilities["request_schema"]["$defs"]["Operation"].is_object());
    assert!(capabilities["response_schema"]["$defs"]["ErrorCode"].is_object());
    let path =
        std::env::temp_dir().join(format!("openwam-api-request-{}.json", std::process::id()));
    std::fs::write(&path, serde_json::to_vec(&envelope(json!({"operation":"catalog","model":"desktop","material":"Aluminum","units":"imperial"}))).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_openwam"))
        .arg("request")
        .arg(&path)
        .output()
        .unwrap();
    std::fs::remove_file(path).unwrap();
    assert!(output.status.success() && output.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["result"]["kind"],
        "catalog"
    );
}

#[test]
fn research_post_is_recomputable_and_rejects_changed_code_or_authority() {
    let (prepared, ok) = invoke(&api::capabilities()["prepare_example"]);
    assert!(ok);
    let project = prepared["result"]["project"].clone();
    let (posted, ok) = invoke(&envelope(
        json!({"operation":"research_gcode", "project":project, "profile":"historical_wam16"}),
    ));
    assert!(ok, "{posted}");
    let candidate = posted["result"]["candidate"].clone();
    assert_eq!(candidate["code"]["machine_output_enabled"], false);
    assert_eq!(candidate["code"]["controller_qualified"], false);
    let (inspection, ok) = invoke(&envelope(
        json!({"operation":"inspect_gcode", "gcode_text":candidate["code"]["gcode"]}),
    ));
    assert!(ok, "{inspection}");
    assert_eq!(
        inspection["result"]["inspection"]["program"],
        candidate["code"]["program"]
    );
    assert_eq!(
        inspection["result"]["inspection"]["summary"]["unresolved_feed_moves"],
        0
    );
    let (verified, ok) = invoke(&envelope(
        json!({"operation":"verify_research_gcode", "project":project, "candidate":candidate}),
    ));
    assert!(ok && verified["result"]["verification"]["verified"] == true);
    for field in [
        "gcode",
        "machine_output_enabled",
        "controller_qualified",
        "blockers",
    ] {
        let mut forged = candidate.clone();
        forged["code"][field] = match field {
            "gcode" => json!(
                candidate["code"][field]
                    .as_str()
                    .unwrap()
                    .replace("M9", "M8")
            ),
            "blockers" => json!([]),
            _ => json!(true),
        };
        let (rejected, ok) = invoke(&envelope(
            json!({"operation":"verify_research_gcode", "project":project, "candidate":forged}),
        ));
        assert!(!ok);
        assert_eq!(rejected["error"]["code"], "invalid_candidate");
    }
    let mut forged = candidate;
    forged["source_sha256"] = json!("forged");
    let (rejected, ok) = invoke(&envelope(
        json!({"operation":"verify_research_gcode", "project":project, "candidate":forged}),
    ));
    assert!(!ok && rejected["error"]["code"] == "invalid_candidate");
}

#[test]
fn research_inspection_rejects_unsupported_machine_commands() {
    let (rejected, ok) = invoke(&envelope(
        json!({"operation":"inspect_gcode", "gcode_text":"G20\nM3\n"}),
    ));
    assert!(!ok && rejected["error"]["code"] == "gcode_invalid");
}

#[test]
fn research_comparison_binds_project_reference_and_scope_and_rejects_tampering() {
    let (prepared, ok) = invoke(&api::capabilities()["prepare_example"]);
    assert!(ok);
    let project = prepared["result"]["project"].clone();
    let (posted, ok) = invoke(&envelope(
        json!({"operation":"research_gcode","project":project,"profile":"historical_wam16"}),
    ));
    assert!(ok);
    // Self comparison exercises deterministic binding, not vendor parity.
    let reference = &posted["result"]["candidate"]["code"]["gcode"];
    let (compared, ok) = invoke(&envelope(
        json!({"operation":"compare_research_gcode","project":project,"profile":"historical_wam16","reference_gcode":reference,
        "tolerances":{"coordinates_mm":0,"feed_mm_min":0,"dwell_seconds":0}}),
    ));
    assert!(ok, "{compared}");
    let comparison = &compared["result"]["comparison"];
    assert_eq!(
        comparison["streams"]["comparison"]["verdict"],
        "equivalent_observed_trace"
    );
    assert_eq!(comparison["source_settings_equivalence_established"], false);
    let (verified, ok) = invoke(&envelope(
        json!({"operation":"verify_research_comparison","project":project,"reference_gcode":reference,"comparison":comparison}),
    ));
    assert!(ok && verified["result"]["verification"]["verified"] == true);
    assert_eq!(
        verified["result"]["verification"]["controller_qualified"],
        false
    );
    for path in [
        "/streams/reference_sha256",
        "/streams/comparison/difference_counts/feed",
        "/machine_output_enabled",
        "/source_settings_equivalence_established",
    ] {
        let mut forged = comparison.clone();
        *forged.pointer_mut(path).unwrap() = match path {
            "/streams/reference_sha256" => json!("forged"),
            "/streams/comparison/difference_counts/feed" => json!(500),
            _ => json!(true),
        };
        let (rejected, ok) = invoke(&envelope(
            json!({"operation":"verify_research_comparison","project":project,"reference_gcode":reference,"comparison":forged}),
        ));
        assert!(!ok && rejected["error"]["code"] == "invalid_comparison");
    }
    let changed_reference = format!("; changed retained bytes\n{}", reference.as_str().unwrap());
    let (rejected, ok) = invoke(&envelope(
        json!({"operation":"verify_research_comparison","project":project,"reference_gcode":changed_reference,"comparison":comparison}),
    ));
    assert!(!ok && rejected["error"]["code"] == "invalid_comparison");
}

#[test]
fn stream_comparison_distinguishes_formatting_from_feeds_and_invalid_code() {
    // Independent protocol fixture, not obtained from WAM or our generator.
    let reference = "G90\nG21\nM1403\nM1405 X0 Y0\nM1406 X100 Y-100\nM1407 S2\nM1410 1.6\nM1411 Aluminum\nM1412 3 mm\nG0 X1 Y-1\nM3\nM8\nG4 S2\nG1 X5 Y-1 F120\nG1 X10 Y-1\nG4 S1\nM9\nG4 S1\nM5\nG4 S1\nM1413 00:00:10\nM1404\n";
    let operation = |candidate: &str| {
        envelope(
            json!({"operation":"compare_gcode","reference_gcode":reference,"candidate_gcode":candidate,
        "tolerances":{"coordinates_mm":0,"feed_mm_min":0,"dwell_seconds":0}}),
        )
    };
    let (matching, ok) = invoke(&operation(&reference.replace("X10 Y-1", "X10.0 Y-1 F120")));
    assert!(ok);
    let comparison = &matching["result"]["comparison"]["comparison"];
    assert_eq!(comparison["verdict"], "equivalent_observed_trace");
    assert_eq!(comparison["byte_identical"], false);
    assert_eq!(comparison["commands_exactly_equal"], false);
    let (different, ok) = invoke(&operation(&reference.replace("F120", "F100")));
    assert!(ok); // A difference is a successful observation, not a transport error.
    assert_eq!(
        different["result"]["comparison"]["comparison"]["difference_counts"]["feed"],
        2
    );
    let (invalid, ok) = invoke(&operation("G20\n"));
    assert!(!ok && invalid["error"]["code"] == "comparison_failed");
}

#[test]
fn current_pro_service_profile_is_explicit_recomputable_and_rejects_desktop() {
    let mut prepare = api::capabilities()["prepare_example"].clone();
    prepare["request"]["source_text"] =
        json!(include_str!("../../../wazer/fixtures/parity-square.svg"));
    prepare["request"]["settings"]["model"] = json!("pro");
    prepare["request"]["settings"]["material"] = json!("Aluminum");
    prepare["request"]["settings"]["thickness"] = json!(3);
    let (prepared, ok) = invoke(&prepare);
    assert!(ok, "{prepared}");
    let project = &prepared["result"]["project"];
    let (posted, ok) = invoke(&envelope(
        json!({"operation":"research_gcode","project":project,"profile":"wam24_pro_presegmented"}),
    ));
    assert!(ok, "{posted}");
    let candidate = &posted["result"]["candidate"];
    assert_eq!(candidate["code"]["summary"]["linear_count"], 5);
    assert_eq!(candidate["code"]["summary"]["declared_version"], "2.4.0");
    assert!(
        !candidate["code"]["machine_output_enabled"]
            .as_bool()
            .unwrap()
    );
    let (verified, ok) = invoke(&envelope(
        json!({"operation":"verify_research_gcode","project":project,"candidate":candidate}),
    ));
    assert!(
        ok && verified["result"]["verification"]["verified"] == true,
        "{verified}"
    );
    prepare["request"]["settings"]["model"] = json!("desktop");
    let (prepared, ok) = invoke(&prepare);
    assert!(ok, "{prepared}");
    let (rejected, ok) = invoke(&envelope(
        json!({"operation":"research_gcode","project":prepared["result"]["project"],"profile":"wam24_pro_presegmented"}),
    ));
    assert!(
        !ok && rejected["error"]["code"] == "postprocessing_failed",
        "{rejected}"
    );
}

#[test]
fn javascript_numeric_spelling_preserves_verification_and_candidate_identity() {
    fn js_numbers(v: &mut Value) {
        match v {
            Value::Number(n)
                if n.as_f64()
                    .is_some_and(|f| f.fract() == 0.0 && f.abs() <= (1u64 << 53) as f64) =>
            {
                *v = json!(n.as_f64().unwrap() as i64)
            }
            Value::Array(a) => a.iter_mut().for_each(js_numbers),
            Value::Object(a) => a.values_mut().for_each(js_numbers),
            _ => {}
        }
    }
    let (prepared, ok) = invoke(&api::capabilities()["prepare_example"]);
    assert!(ok);
    let mut project = prepared["result"]["project"].clone();
    let (posted, ok) = invoke(&envelope(
        json!({"operation":"research_gcode","project":project,"profile":"historical_wam16"}),
    ));
    assert!(ok);
    let candidate = posted["result"]["candidate"].clone();
    let (simulated, ok) = invoke(&envelope(
        json!({"operation":"simulate","project":project,"fault":{"kind":"none","at_ms":0}}),
    ));
    assert!(ok, "{simulated}");
    let mut replay = simulated["result"]["replay"].clone();
    js_numbers(&mut replay);
    js_numbers(&mut project);
    let (verified_replay, ok) = invoke(&envelope(
        json!({"operation":"verify_simulation","project":project,"replay":replay}),
    ));
    assert!(ok, "{verified_replay}");
    let (verified, ok) = invoke(&envelope(
        json!({"operation":"verify_research_gcode","project":project,"candidate":candidate}),
    ));
    assert!(ok, "{verified}");
    let mut forged = project.clone();
    forged["integration"]["planning"]["feed_mm_min"] = json!(108.00000000000001);
    let (rejected, ok) = invoke(&envelope(json!({"operation":"verify","project":forged})));
    assert!(!ok && rejected["error"]["code"] == "invalid_project");
}

#[test]
fn independent_cam_imports_anonymous_contours_retains_intent_and_rejects_tampering() {
    let mut prepare = api::capabilities()["prepare_example"].clone();
    prepare["request"]["source_text"] = json!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100"><path d="M10 10H90V90H10Z"/><path d="M30 30H50V50H30Z"/></svg>"#
    );
    let (prepared, ok) = invoke(&prepare);
    assert!(ok, "{prepared}");
    let project = &prepared["result"]["project"];
    let intent = json!({"kerf_override_mm":null,"contours":[{"source_id":"svg-contour-0","side":"outside","start_fraction":0,"tabs":[{"center_fraction":0.125,"width_mm":3}],"lead_in_mm":2,"lead_out_mm":2},{"source_id":"svg-contour-1","side":"inside","start_fraction":0,"tabs":[],"lead_in_mm":2,"lead_out_mm":2}]});
    let (planned, ok) = invoke(&envelope(
        json!({"operation":"plan_cam","project":project,"intent":intent}),
    ));
    assert!(ok, "{planned}");
    let artifact = &planned["result"]["artifact"];
    assert_eq!(artifact["plan"]["kerf_basis"], "request_published_nominal");
    assert_eq!(artifact["plan"]["kerf_mm"], 1.1);
    assert_eq!(artifact["plan"]["paths"][0]["source_id"], "svg-contour-1");
    assert_eq!(
        artifact["plan"]["paths"][1]["cut_spans_mm"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let (verified, ok) = invoke(&envelope(
        json!({"operation":"verify_cam","artifact":artifact}),
    ));
    assert!(
        ok && verified["result"]["verification"]["verified"] == true,
        "{verified}"
    );
    let (preview, ok) = invoke(&envelope(
        json!({"operation":"preview_cam","artifact":artifact}),
    ));
    assert!(ok, "{preview}");
    let svg = preview["result"]["content"].as_str().unwrap();
    assert!(roxmltree::Document::parse(svg).is_ok());
    assert!(svg.contains("stroke-dasharray"));
    assert!(!svg.contains("<script"));
    for pointer in [
        "/plan/kerf_mm",
        "/intent/contours/0/tabs/0/width_mm",
        "/machine_output_enabled",
    ] {
        let mut forged = artifact.clone();
        *forged.pointer_mut(pointer).unwrap() = if pointer == "/machine_output_enabled" {
            json!(true)
        } else {
            json!(7)
        };
        let (rejected, ok) = invoke(&envelope(
            json!({"operation":"verify_cam","artifact":forged}),
        ));
        assert!(
            !ok && rejected["error"]["code"] == "cam_invalid",
            "{rejected}"
        );
    }
    let mut invalid = intent.clone();
    invalid["contours"][1]["lead_in_mm"] = json!(25);
    let (rejected, ok) = invoke(&envelope(
        json!({"operation":"plan_cam","project":project,"intent":invalid}),
    ));
    assert!(!ok && rejected["error"]["code"] == "cam_invalid");
}

#[test]
fn cam_panel_fixture_compound_curves_and_annotations_rebuild_with_explicit_kerf() {
    let mut prepare = api::capabilities()["prepare_example"].clone();
    prepare["request"]["source_text"] =
        json!(include_str!("../../../wazer/fixtures/cam-panel.svg"));
    let (prepared, ok) = invoke(&prepare);
    assert!(ok, "{prepared}");
    let project = &prepared["result"]["project"];
    let mut intent: Value = serde_json::from_str(include_str!(
        "../../../wazer/fixtures/cam-panel.intent.json"
    ))
    .unwrap();
    let request = envelope(json!({"operation":"plan_cam","project":project,"intent":intent}));
    let (planned, ok) = invoke(&request);
    assert!(ok, "{planned}");
    let (repeated, ok) = invoke(&request);
    assert!(ok && repeated == planned);
    let artifact = &planned["result"]["artifact"];
    let paths = artifact["plan"]["paths"].as_array().unwrap();
    assert_eq!(paths.len(), 4);
    assert_eq!(paths[2]["source_id"], "round-opening");
    assert_eq!(paths[3]["uncut_intervals_mm"].as_array().unwrap().len(), 2);
    assert_eq!(paths[3]["cut_spans_mm"].as_array().unwrap().len(), 3);
    assert_eq!(
        artifact["plan"]["draft"]["operations"]
            .as_array()
            .unwrap()
            .len(),
        108
    );
    let (verified, ok) = invoke(&envelope(
        json!({"operation":"verify_cam","artifact":artifact}),
    ));
    assert!(ok && verified["result"]["verification"]["verified"] == true);
    let (preview, ok) = invoke(&envelope(
        json!({"operation":"preview_cam","artifact":artifact}),
    ));
    assert!(ok, "{preview}");
    let doc = roxmltree::Document::parse(preview["result"]["content"].as_str().unwrap()).unwrap();
    let annotation = doc
        .descendants()
        .find(|n| {
            n.has_tag_name("polyline")
                && n.children()
                    .any(|c| c.has_tag_name("title") && c.text() == Some("annotation"))
        })
        .unwrap();
    assert_eq!(
        annotation
            .attribute("points")
            .unwrap()
            .split_whitespace()
            .count(),
        2
    );
    intent["kerf_override_mm"] = json!(0.9);
    let (overridden, ok) = invoke(&envelope(
        json!({"operation":"plan_cam","project":project,"intent":intent}),
    ));
    assert!(ok, "{overridden}");
    let changed = &overridden["result"]["artifact"];
    assert_eq!(changed["plan"]["kerf_mm"], 0.9);
    assert_eq!(changed["plan"]["kerf_basis"], "explicit_override");
    assert_ne!(changed["plan"]["paths"], artifact["plan"]["paths"]);
    let (verified, ok) = invoke(&envelope(
        json!({"operation":"verify_cam","artifact":changed}),
    ));
    assert!(ok && verified["result"]["verification"]["verified"] == true);
}
