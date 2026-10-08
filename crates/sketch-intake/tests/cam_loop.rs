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

fn linuxcnc_profile() -> Value {
    json!({"digital_output_count":4,"jet_output":0,"abrasive_output":1,
        "jet_settle_seconds":0.5,"abrasive_flush_seconds":0.25,
        "x_limits_mm":[0,460],"y_limits_mm":[-305,0],"max_feed_mm_min":1500})
}

#[test]
fn linuxcnc_export_preserves_compensated_spans_and_verifies_source_bound_output() {
    let artifact = artifact();
    let candidate = ok(
        json!({"operation":"export_cam_linuxcnc","artifact":artifact,
        "profile":linuxcnc_profile()}),
    )["candidate"]
        .clone();
    let operations = artifact["plan"]["draft"]["operations"].as_array().unwrap();
    let code = candidate["code"]["gcode"].as_str().unwrap();
    let motion: Vec<_> = code
        .lines()
        .filter(|line| line.starts_with("G0 ") || line.starts_with("G1 "))
        .collect();
    let expected: Vec<_> = operations
        .iter()
        .filter(|op| op["kind"] == "rapid" || op["kind"] == "cut")
        .collect();
    assert_eq!(motion.len(), expected.len());
    for (line, op) in motion.iter().zip(expected) {
        let words: Vec<_> = line.split_whitespace().collect();
        assert_eq!(words[0], if op["kind"] == "rapid" { "G0" } else { "G1" });
        for (axis, index) in [('X', 0), ('Y', 1)] {
            let value = words
                .iter()
                .find_map(|word| word.strip_prefix(axis))
                .unwrap()
                .parse::<f64>()
                .unwrap();
            assert!((value - op["to_mm"][index].as_f64().unwrap()).abs() <= 0.000000006);
        }
        if op["kind"] == "cut" {
            let feed = words
                .iter()
                .find_map(|word| word.strip_prefix('F'))
                .unwrap()
                .parse::<f64>()
                .unwrap();
            assert!((feed - op["feed_mm_min"].as_f64().unwrap()).abs() <= 0.000000006);
        }
    }
    let spans = operations
        .iter()
        .filter(|op| op["kind"] == "pierce")
        .count();
    assert!(
        spans > artifact["plan"]["paths"].as_array().unwrap().len(),
        "tabs must produce separate pierced spans"
    );
    assert_eq!(candidate["code"]["span_count"], spans);
    assert_eq!(code.lines().filter(|line| *line == "M64 P0").count(), spans);
    assert_eq!(
        code.lines().filter(|line| *line == "M65 P0").count(),
        spans + 1
    );
    assert_eq!(candidate["machine_output_enabled"], false);
    let verified =
        ok(json!({"operation":"verify_cam_linuxcnc","artifact":artifact,"candidate":candidate}));
    assert_eq!(verified["verification"]["verified"], true);
    for pointer in [
        "/code/gcode",
        "/gcode_sha256",
        "/cam_sha256",
        "/machine_output_enabled",
        "/code/controller_qualified",
        "/code/profile/jet_output",
    ] {
        let mut forged = candidate.clone();
        *forged.pointer_mut(pointer).unwrap() = match pointer {
            "/machine_output_enabled" | "/code/controller_qualified" => json!(true),
            "/code/profile/jet_output" => json!(2),
            _ => json!("forged"),
        };
        let (response, success) = invoke(
            json!({"operation":"verify_cam_linuxcnc","artifact":artifact,"candidate":forged}),
        );
        assert!(
            !success && response["error"]["code"] == "invalid_candidate",
            "{response}"
        );
    }
    let mut forged = artifact.clone();
    forged["plan"]["kerf_mm"] = json!(9.0);
    assert!(!invoke(json!({"operation":"export_cam_linuxcnc","artifact":forged,"profile":linuxcnc_profile()})).1);
    let mut profile = linuxcnc_profile();
    profile["abrasive_output"] = json!(0);
    assert!(
        !invoke(json!({"operation":"export_cam_linuxcnc","artifact":artifact,"profile":profile})).1
    );
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

fn exported(artifact: &Value) -> Value {
    ok(json!({"operation":"research_cam_gcode","artifact":artifact,"profile":"historical_wam16"}))
        ["candidate"]
        .clone()
}
fn command_replay(artifact: &Value, candidate: &Value, kind: &str, at_ms: u64) -> Value {
    ok(json!({"operation":"simulate_cam_gcode","artifact":artifact,"candidate":candidate,"fault":{"kind":kind,"at_ms":at_ms}}))["replay"].clone()
}
fn verify_commands(artifact: &Value, candidate: &Value, replay: &Value) -> (Value, bool) {
    invoke(
        json!({"operation":"verify_cam_gcode_simulation","artifact":artifact,"candidate":candidate,"replay":replay}),
    )
}

fn completion_settings() -> Value {
    let example: Value = serde_json::from_str(include_str!(
        "../../../wazer/fixtures/shutdown-completion.request.json"
    ))
    .unwrap();
    json!({"config_at_dispatch":example["request"]["input"]["config"],
        "output_fault":{"kind":"none"},"poll_offsets_us":[0,1000000,1000001]})
}

#[test]
fn exported_completion_keeps_emitted_dwells_and_internal_timers_separate() {
    let artifact = artifact();
    let candidate = exported(&artifact);
    let completion = completion_settings();
    let fault = json!({"kind":"none","at_ms":0});
    let baseline = command_replay(&artifact, &candidate, "none", 0);
    let replay = ok(
        json!({"operation":"simulate_cam_gcode_shutdown","artifact":artifact,
        "candidate":candidate,"fault":fault,"completion":completion}),
    )["replay"]
        .clone();
    assert_eq!(replay["command_replay"], baseline);
    assert_eq!(replay["completion_status"], "dispatched");
    assert_eq!(replay["completion"]["dispatch_command_index"], 154);
    assert_eq!(replay["completion"]["dispatch_at_program_us"], 383650000);
    assert_eq!(
        replay["completion"]["last_observation_at_program_us"],
        384650001
    );
    assert_eq!(replay["command_replay"]["trace"]["dwell_elapsed_ms"], 54000);
    assert_eq!(
        replay["completion"]["relative_replay"]["final_state"],
        "all_done"
    );
    assert!(
        replay["completion"]["relative_replay"]["trace"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["at_us"] == 1000000 && entry["action"]["kind"] == "timer_waiting")
    );
    let verify = |replay: Value| {
        invoke(
            json!({"operation":"verify_cam_gcode_shutdown", "artifact":artifact,
        "candidate":candidate,"fault":fault,"completion":completion,"replay":replay}),
        )
    };
    assert!(verify(replay.clone()).1);
    let mut forged = replay;
    forged["completion"]["dispatch_at_program_us"] = json!(0);
    let (rejected, success) = verify(forged);
    assert!(!success && rejected["error"]["code"] == "invalid_replay");
}

#[test]
fn interrupted_programs_do_not_invent_completion_and_output_faults_stay_visible() {
    let artifact = artifact();
    let candidate = exported(&artifact);
    let mut completion = completion_settings();
    let replay = ok(json!({"operation":"simulate_cam_gcode_shutdown","artifact":artifact,
        "candidate":candidate,"fault":{"kind":"enclosure_open","at_ms":10000},"completion":completion}))["replay"].clone();
    assert_eq!(replay["completion_status"], "not_dispatched");
    assert!(replay["completion"].is_null());
    assert_eq!(replay["command_replay"]["trace"]["outcome"], "faulted");
    completion["output_fault"] =
        json!({"kind":"hold_electrical","channel":"high_pressure_pump","level":true});
    let replay = ok(
        json!({"operation":"simulate_cam_gcode_shutdown","artifact":artifact,
        "candidate":candidate,"fault":{"kind":"none","at_ms":0},"completion":completion}),
    )["replay"]
        .clone();
    assert_eq!(
        replay["completion"]["relative_replay"]["final_state"],
        "all_done"
    );
    assert_eq!(
        replay["completion"]["relative_replay"]["final_output_mismatches"],
        json!(["high_pressure_pump"])
    );
    completion["poll_offsets_us"] = json!([0, 1000000]);
    let replay = ok(
        json!({"operation":"simulate_cam_gcode_shutdown","artifact":artifact,
        "candidate":candidate,"fault":{"kind":"none","at_ms":0},"completion":completion}),
    )["replay"]
        .clone();
    assert_eq!(
        replay["completion"]["relative_replay"]["final_state"],
        "completed_cut"
    );
    completion["poll_offsets_us"] = json!([2, 1]);
    assert!(!invoke(json!({"operation":"simulate_cam_gcode_shutdown","artifact":artifact,
        "candidate":candidate,"fault":{"kind":"enclosure_open","at_ms":10000},"completion":completion})).1);
}

#[test]
fn exported_commands_replay_exact_dwells_moves_and_separate_output_channels() {
    let artifact = artifact();
    let candidate = exported(&artifact);
    let replay = command_replay(&artifact, &candidate, "none", 0);
    assert_eq!(replay, command_replay(&artifact, &candidate, "none", 0));
    assert!(verify_commands(&artifact, &candidate, &replay).1);
    let trace = &replay["trace"];
    assert_eq!(trace["outcome"], "completed");
    assert_eq!(trace["command_count"], 155);
    assert_eq!(trace["completed_commands"], 155);
    assert_eq!(trace["dwell_elapsed_ms"], 54_000);
    let expected_length = artifact["plan"]["draft"]["raw_centerline_length_mm"]
        .as_f64()
        .unwrap();
    assert!((trace["simulated_cut_length_mm"].as_f64().unwrap() - expected_length).abs() < 1e-8);
    let frames = trace["frames"].as_array().unwrap();
    let commands = candidate["code"]["program"]["commands"].as_array().unwrap();
    let mut position = [0.0_f64, 0.0];
    let mut feed = 0.0;
    let mut elapsed = 0_u64;
    let rapid_feed = trace["model"]["rapid_feed_mm_min"].as_f64().unwrap();
    for (i, command) in commands.iter().enumerate() {
        let start = frames
            .iter()
            .find(|f| f["observation"]["operation"] == i && f["observation"]["phase"] == "start")
            .unwrap();
        let end = frames
            .iter()
            .find(|f| f["observation"]["operation"] == i && f["observation"]["phase"] == "end")
            .unwrap();
        assert_eq!(&end["command"], command);
        assert_eq!(start["observation"]["at_ms"], elapsed);
        match command["command"].as_str().unwrap() {
            "rapid" | "linear" => {
                let target = [
                    command["xy_mm"][0].as_f64().unwrap(),
                    command["xy_mm"][1].as_f64().unwrap(),
                ];
                if let Some(value) = command["feed_mm_min"].as_f64() {
                    feed = value;
                }
                let rate = if command["command"] == "rapid" {
                    rapid_feed
                } else {
                    feed
                };
                elapsed += ((target[0] - position[0]).hypot(target[1] - position[1]) / rate
                    * 60000.0)
                    .ceil() as u64;
                position = target;
                assert_eq!(end["observation"]["position_mm"], command["xy_mm"]);
                if command["command"] == "rapid" {
                    assert_eq!(end["observation"]["effective_jet"], false);
                    assert_eq!(end["observation"]["nozzle_flow"], false);
                }
            }
            "dwell" => {
                elapsed += (command["seconds"].as_f64().unwrap() * 1000.0).ceil() as u64;
                assert_eq!(
                    start["observation"]["position_mm"],
                    end["observation"]["position_mm"]
                );
                assert_eq!(end["channels"]["effective_cutting_motion"], false);
                if i > 0 && commands[i - 1]["command"] == "abrasive_off" {
                    assert_eq!(end["observation"]["effective_jet"], true);
                    assert_eq!(end["channels"]["effective_abrasive"], false);
                    assert_eq!(
                        end["observation"]["pressure_units"],
                        trace["model"]["pressure_capacity_units"]
                    );
                }
            }
            "jet_on" | "abrasive_off" => {
                assert_eq!(end["observation"]["effective_jet"], true);
                assert_eq!(end["channels"]["effective_abrasive"], false);
            }
            "abrasive_on" => {
                assert_eq!(end["observation"]["effective_jet"], true);
                assert_eq!(end["channels"]["effective_abrasive"], true);
            }
            "jet_off" => {
                assert_eq!(end["observation"]["effective_jet"], false);
                assert_eq!(end["channels"]["effective_abrasive"], false);
                assert!(end["observation"]["pressure_units"].as_u64().unwrap() > 0);
                let release = frames
                    .iter()
                    .find(|f| {
                        f["observation"]["operation"] == i + 1 && f["observation"]["phase"] == "end"
                    })
                    .unwrap();
                assert_eq!(release["observation"]["pressure_units"], 0);
                assert_eq!(release["observation"]["state"], "Disarmed");
            }
            _ => (),
        }
        assert_eq!(end["observation"]["at_ms"], elapsed);
    }
    assert_eq!(trace["virtual_elapsed_ms"], elapsed);
    for flag in [
        "machine_output_enabled",
        "controller_qualified",
        "hardware_qualified",
    ] {
        assert_eq!(replay[flag], false);
    }
}

#[test]
fn command_faults_and_stalled_host_stop_without_dispatching_more_commands() {
    let artifact = artifact();
    let candidate = exported(&artifact);
    for (kind, at, expected) in [
        ("enclosure_open", 10000, "faulted"),
        ("watchdog_stall", 10000, "faulted"),
        ("stale_input", 10000, "faulted"),
        ("release_blocked", 0, "waiting_for_release"),
        ("pressure_sensor_lies", 0, "unsafe_plant"),
    ] {
        let replay = command_replay(&artifact, &candidate, kind, at);
        assert_eq!(replay["trace"]["outcome"], expected, "{replay}");
        assert!(replay["trace"]["completed_commands"].as_u64().unwrap() < 155);
        assert!(verify_commands(&artifact, &candidate, &replay).1);
    }
    let stalled = command_replay(&artifact, &candidate, "watchdog_stall", 0);
    assert_eq!(stalled["trace"]["completed_commands"], 0);
    assert_eq!(stalled["trace"]["virtual_elapsed_ms"], 100);
    // A fault during the explicit jet-only shutdown dwell must remain timed
    // against exported commands rather than the shorter CAM replay.
    let nominal = command_replay(&artifact, &candidate, "none", 0);
    let off = nominal["trace"]["frames"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["command"]["command"] == "abrasive_off" && f["observation"]["phase"] == "end")
        .unwrap();
    let at = off["observation"]["at_ms"].as_u64().unwrap() + 500;
    let interrupted = command_replay(&artifact, &candidate, "enclosure_open", at);
    assert_eq!(interrupted["trace"]["virtual_elapsed_ms"], at);
    let end = interrupted["trace"]["frames"]
        .as_array()
        .unwrap()
        .last()
        .unwrap();
    assert_eq!(end["command"]["command"], "dwell");
    assert_eq!(end["observation"]["effective_jet"], false);
    assert_eq!(end["channels"]["effective_abrasive"], false);
    assert!(verify_commands(&artifact, &candidate, &interrupted).1);
}

#[test]
fn command_replay_rejects_forged_export_trace_and_changed_source_intent() {
    let artifact = artifact();
    let candidate = exported(&artifact);
    let replay = command_replay(&artifact, &candidate, "none", 0);
    for pointer in [
        "/gcode_sha256",
        "/trace/dwell_elapsed_ms",
        "/trace/frames/0/channels/effective_abrasive",
        "/machine_output_enabled",
    ] {
        let mut forged = replay.clone();
        let v = forged.pointer_mut(pointer).unwrap();
        *v = if v.is_boolean() {
            json!(true)
        } else if v.is_number() {
            json!(0)
        } else {
            json!("forged")
        };
        let (response, success) = verify_commands(&artifact, &candidate, &forged);
        assert!(!success && response["error"]["code"] == "invalid_replay");
    }
    let mut forged = candidate.clone();
    forged["code"]["gcode"] = json!(
        candidate["code"]["gcode"]
            .as_str()
            .unwrap()
            .replacen("G4 S1", "G4 S2", 1)
    );
    assert_ne!(forged, candidate);
    assert!(!invoke(json!({"operation":"simulate_cam_gcode","artifact":artifact,"candidate":forged,"fault":{"kind":"none","at_ms":0}})).1);
    let mut revised = artifact.clone();
    revised["intent"]["contours"][4]["start_fraction"] = json!(0.5);
    ok(json!({"operation":"verify_cam","artifact":revised}));
    assert!(!verify_commands(&revised, &candidate, &replay).1);
    assert!(!invoke(json!({"operation":"simulate_cam_gcode","artifact":artifact,"candidate":candidate,"fault":{"kind":"enclosure_open","at_ms":3600000}})).1);
}
