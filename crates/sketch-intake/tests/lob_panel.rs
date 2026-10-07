use reversal_sketch_intake::{
    inspect_svg,
    openwam_api::{self as api, Response},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn request(operation: Value) -> Value {
    let response = api::dispatch(
        &serde_json::to_vec(&json!({"schema":api::REQUEST_SCHEMA,"request":operation})).unwrap(),
    );
    assert!(matches!(response, Response::Ok { .. }), "{response:?}");
    serde_json::to_value(response).unwrap()
}

#[test]
fn actual_lob_panel_artifact_preserves_six_closed_contours_through_agent_workflow() {
    // Produced by the owner CLI, not hand-reconstructed DXF or SVG. Owner
    // verification is exercised in that package and in the acquisition runbook.
    let panel: Value = serde_json::from_str(include_str!(
        "../../../wazer/fixtures/lob-8hp.panel-cut.json"
    ))
    .unwrap();
    let svg = panel["svg"].as_str().unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(svg.as_bytes())),
        panel["svg_sha256"]
    );
    let sketch = inspect_svg(svg.as_bytes()).unwrap();
    assert!((sketch.viewport_mm[0] - 40.64).abs() < 0.0001);
    assert!((sketch.viewport_mm[1] - 128.5).abs() < 0.0001);
    assert_eq!(sketch.contours.len(), 6);
    assert!(sketch.contours.iter().all(|c| c.closed));
    assert_eq!(sketch.contours.last().unwrap().source_id, "panel-outline");
    for (contour, intent) in sketch
        .contours
        .iter()
        .zip(panel["contours"].as_array().unwrap())
    {
        assert_eq!(contour.source_id, intent["id"]);
        assert_eq!(intent["mechanically_verified"], false);
    }
    let prepared = request(
        json!({"operation":"prepare","source_text":svg,"source_format":"svg",
        "settings":{"model":"pro","material":"Aluminum","units":"metric","thickness":panel["thickness_mm"],"tolerance_mm":0.02,"placement_xy_down_mm":[0,0]}}),
    );
    let project = &prepared["result"]["project"];
    assert_eq!(project["source_text"], svg);
    let operations = project["integration"]["swarf"]["operations"]
        .as_array()
        .unwrap();
    assert_eq!(
        operations
            .iter()
            .filter(|op| op["kind"] == "pierce")
            .count(),
        6
    );
    assert_eq!(
        operations.iter().filter(|op| op["kind"] == "stop").count(),
        6
    );
    let verified = request(json!({"operation":"verify","project":project}));
    assert_eq!(verified["result"]["verification"]["verified"], true);
    let posted = request(
        json!({"operation":"research_gcode","project":project,"profile":"historical_wam16"}),
    );
    let candidate = &posted["result"]["candidate"];
    assert_eq!(candidate["code"]["summary"]["path_count"], 6);
    assert_eq!(candidate["code"]["summary"]["linear_count"], 216);
    assert_eq!(candidate["code"]["summary"]["unresolved_feed_moves"], 0);
    let moves: Vec<_> = candidate["code"]["program"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["command"] == "linear")
        .collect();
    for (movement, operation) in moves
        .iter()
        .zip(operations.iter().filter(|op| op["kind"] == "cut"))
    {
        assert_eq!(movement["xy_mm"], operation["to_mm"]);
        assert_eq!(movement["feed_mm_min"], operation["feed_mm_min"]);
    }
    let code_verified = request(
        json!({"operation":"verify_research_gcode","project":project,"candidate":candidate}),
    );
    assert_eq!(code_verified["result"]["verification"]["verified"], true);
    assert_eq!(
        code_verified["result"]["verification"]["controller_qualified"],
        false
    );
    let replay = request(
        json!({"operation":"simulate","project":project,"fault":{"kind":"none","at_ms":0}}),
    );
    assert_eq!(replay["result"]["replay"]["outcome"], "completed");
    assert_eq!(
        replay["result"]["replay"]["completed_operations"],
        replay["result"]["replay"]["operation_count"]
    );
    let replay_verified = request(
        json!({"operation":"verify_simulation","project":project,"replay":replay["result"]["replay"]}),
    );
    assert_eq!(replay_verified["result"]["verification"]["verified"], true);
    assert_eq!(replay_verified["machine_output_enabled"], false);
    let faulted = request(
        json!({"operation":"simulate","project":project,"fault":{"kind":"enclosure_open","at_ms":10000}}),
    );
    assert_eq!(faulted["result"]["replay"]["outcome"], "faulted");
    let fault_verified = request(
        json!({"operation":"verify_simulation","project":project,"replay":faulted["result"]["replay"]}),
    );
    assert_eq!(fault_verified["result"]["verification"]["verified"], true);
}
