use reversal_sketch_intake::openwam_api::{self as api};
use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn invoke(operation: Value) -> (Value, bool) {
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
        .write_all(
            &serde_json::to_vec(&json!({"schema":api::REQUEST_SCHEMA,"request":operation}))
                .unwrap(),
        )
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    (
        serde_json::from_slice(&out.stdout).unwrap(),
        out.status.success(),
    )
}
fn ok(operation: Value) -> Value {
    let (response, success) = invoke(operation);
    assert!(success, "{response}");
    response["result"].clone()
}
fn artifact() -> Value {
    let mut prepare = api::capabilities()["prepare_example"]["request"].clone();
    prepare["source_text"] = json!(include_str!("../../../wazer/fixtures/cam-panel.svg"));
    let project = ok(prepare)["project"].clone();
    let intent: Value = serde_json::from_str(include_str!(
        "../../../wazer/fixtures/cam-panel.intent.json"
    ))
    .unwrap();
    ok(json!({"operation":"plan_cam","project":project,"intent":intent}))["artifact"].clone()
}

#[test]
fn panel_cam_replay_and_export_preserve_every_span_cut_and_tab_gap() {
    let artifact = artifact();
    let operations = artifact["plan"]["draft"]["operations"].as_array().unwrap();
    let replay = ok(
        json!({"operation":"simulate_cam","artifact":artifact,"fault":{"kind":"none","at_ms":0}}),
    )["replay"]
        .clone();
    assert_eq!(replay["replay"]["outcome"], "completed");
    assert_eq!(replay["replay"]["completed_operations"], operations.len());
    assert_eq!(replay["machine_output_enabled"], false);
    let actual_length = replay["replay"]["simulated_cut_length_mm"]
        .as_f64()
        .unwrap();
    let expected_length = artifact["plan"]["draft"]["raw_centerline_length_mm"]
        .as_f64()
        .unwrap();
    let perimeters: f64 = artifact["plan"]["paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["perimeter_mm"].as_f64().unwrap())
        .sum();
    // Two 3 mm gaps, twelve mm of outer-span leads, eight mm of rectangular
    // hole leads and two mm of round-hole leads. No cut length across bridges.
    assert!((expected_length - (perimeters - 6.0 + 22.0)).abs() < 1e-8);
    assert!((actual_length - expected_length).abs() < 1e-8);
    let frames = replay["replay"]["frames"].as_array().unwrap();
    for (index, operation) in operations.iter().enumerate() {
        let end = frames
            .iter()
            .find(|f| f["operation"] == index && f["phase"] == "end")
            .unwrap();
        if operation["kind"] == "rapid" || operation["kind"] == "cut" {
            assert_eq!(end["position_mm"], operation["to_mm"]);
        }
        if operation["kind"] == "rapid" {
            assert_eq!(end["state"], "Disarmed");
            assert_eq!(end["effective_jet"], false);
            assert_eq!(end["nozzle_flow"], false);
        }
    }
    assert_eq!(
        ok(json!({"operation":"verify_cam_simulation","artifact":artifact,"replay":replay}))["verification"]
            ["verified"],
        true
    );
    let candidate = ok(
        json!({"operation":"research_cam_gcode","artifact":artifact,"profile":"historical_wam16"}),
    )["candidate"]
        .clone();
    assert_eq!(candidate["cam_sha256"], replay["cam_sha256"]);
    let inspection =
        ok(json!({"operation":"inspect_gcode","gcode_text":candidate["code"]["gcode"]}));
    let commands = inspection["inspection"]["program"]["commands"]
        .as_array()
        .unwrap();
    let moves: Vec<_> = commands
        .iter()
        .filter(|c| c["command"] == "rapid" || c["command"] == "linear")
        .collect();
    let expected_moves: Vec<_> = operations
        .iter()
        .filter(|o| o["kind"] == "rapid" || o["kind"] == "cut")
        .collect();
    assert_eq!(moves.len(), expected_moves.len());
    for (command, operation) in moves.iter().zip(expected_moves) {
        assert_eq!(command["xy_mm"], operation["to_mm"]);
        assert_eq!(command["command"] == "rapid", operation["kind"] == "rapid");
        if command["command"] == "linear" {
            assert_eq!(command["feed_mm_min"], operation["feed_mm_min"]);
        }
    }
    assert_eq!(
        commands.iter().filter(|c| c["command"] == "jet_on").count(),
        6
    );
    assert_eq!(
        commands
            .iter()
            .filter(|c| c["command"] == "jet_off")
            .count(),
        6
    );
    assert_eq!(
        ok(json!({"operation":"verify_cam_gcode","artifact":artifact,"candidate":candidate}))["verification"]
            ["verified"],
        true
    );
}

#[test]
fn cam_faults_stop_motion_and_replays_are_independently_recomputed() {
    let artifact = artifact();
    for kind in [
        "enclosure_open",
        "watchdog_stall",
        "stale_input",
        "release_blocked",
        "pressure_sensor_lies",
    ] {
        let at = if kind == "release_blocked" || kind == "pressure_sensor_lies" {
            0
        } else {
            10000
        };
        let replay = ok(json!({"operation":"simulate_cam","artifact":artifact,"fault":{"kind":kind,"at_ms":at}}))["replay"].clone();
        assert_ne!(replay["replay"]["outcome"], "completed");
        assert!(replay["replay"]["completed_operations"].as_u64().unwrap() < 108);
        assert_eq!(
            ok(json!({"operation":"verify_cam_simulation","artifact":artifact,"replay":replay}))["verification"]
                ["verified"],
            true
        );
        if kind == "enclosure_open" {
            assert_eq!(replay["replay"]["virtual_elapsed_ms"], 10000);
            assert_eq!(
                replay["replay"]["frames"]
                    .as_array()
                    .unwrap()
                    .last()
                    .unwrap()["effective_jet"],
                false
            );
        }
    }
}

#[test]
fn changed_intent_outputs_authority_and_unsupported_profiles_fail() {
    let artifact = artifact();
    let replay = ok(
        json!({"operation":"simulate_cam","artifact":artifact,"fault":{"kind":"none","at_ms":0}}),
    )["replay"]
        .clone();
    let candidate = ok(
        json!({"operation":"research_cam_gcode","artifact":artifact,"profile":"historical_wam16"}),
    )["candidate"]
        .clone();
    for pointer in [
        "/cam_sha256",
        "/replay/frames/0/position_mm/0",
        "/machine_output_enabled",
    ] {
        let mut changed = replay.clone();
        *changed.pointer_mut(pointer).unwrap() = match pointer {
            "/cam_sha256" => json!("forged"),
            "/machine_output_enabled" => json!(true),
            _ => json!(999),
        };
        let (r, success) = invoke(
            json!({"operation":"verify_cam_simulation","artifact":artifact,"replay":changed}),
        );
        assert!(!success && r["error"]["code"] == "invalid_replay", "{r}");
    }
    for pointer in [
        "/cam_sha256",
        "/code/gcode",
        "/code/controller_qualified",
        "/machine_output_enabled",
    ] {
        let mut changed = candidate.clone();
        *changed.pointer_mut(pointer).unwrap() =
            if pointer.ends_with("qualified") || pointer.ends_with("enabled") {
                json!(true)
            } else {
                json!("forged")
            };
        let (r, success) =
            invoke(json!({"operation":"verify_cam_gcode","artifact":artifact,"candidate":changed}));
        assert!(!success && r["error"]["code"] == "invalid_candidate", "{r}");
    }
    // Change valid no-cut intent without changing the toolpath. The binding must
    // still refuse evidence created under the previous intent.
    let mut revised = artifact.clone();
    revised["intent"]["contours"][4]["start_fraction"] = json!(0.5);
    ok(json!({"operation":"verify_cam","artifact":revised}));
    assert!(
        !invoke(json!({"operation":"verify_cam_gcode","artifact":revised,"candidate":candidate})).1
    );
    assert!(
        !invoke(json!({"operation":"verify_cam_simulation","artifact":revised,"replay":replay})).1
    );
    assert!(!invoke(json!({"operation":"research_cam_gcode","artifact":artifact,"profile":"wam24_pro_presegmented"})).1);
    assert!(!invoke(json!({"operation":"simulate_cam","artifact":artifact,"fault":{"kind":"enclosure_open","at_ms":3600000}})).1);
}
