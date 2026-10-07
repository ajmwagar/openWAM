use reversal_sketch_intake::{
    bridge::{self, Selection, SourceFormat},
    cutting::{self, Model, Units},
    simulation::{self, FaultKind, FaultSpec, Outcome},
};

fn source() -> &'static [u8] {
    br#"<svg xmlns="http://www.w3.org/2000/svg" width="20mm" height="20mm" viewBox="0 0 20 20"><path id="line" d="M1 1L11 1"/></svg>"#
}
fn integration() -> Vec<u8> {
    serde_json::to_vec(
        &bridge::integrate_source(
            source(),
            SourceFormat::Svg,
            &cutting::bundled().unwrap(),
            Selection {
                model: Model::Pro,
                material: "Aluminum",
                units: Units::Metric,
                thickness: 3.0,
            },
            0.02,
            [0.0, 0.0],
        )
        .unwrap(),
    )
    .unwrap()
}
fn run(kind: FaultKind, at_ms: u64) -> simulation::Replay {
    simulation::replay(
        source(),
        &integration(),
        &cutting::bundled().unwrap(),
        FaultSpec { kind, at_ms },
    )
    .unwrap()
}

#[test]
fn verified_nominal_job_replays_motion_pierce_and_observed_shutdown() {
    let r = run(FaultKind::None, 0);
    assert_eq!(r.outcome, Outcome::Completed);
    assert_eq!(r.completed_operations, 4);
    assert_eq!(r.virtual_elapsed_ms, 11713); // rapid ceil(56.57) + pierce6000 + cut ceil(5555.56) + decay100
    assert!((r.simulated_cut_length_mm - 10.0).abs() < 1e-5); // upstream SVG normalization uses float32
    let last = r.frames.last().unwrap();
    assert!((last.position_mm[0] - 11.0).abs() < 1e-5 && (last.position_mm[1] + 1.0).abs() < 1e-5);
    assert_eq!(last.state, "Disarmed");
    assert_eq!(last.pressure_units, 0);
    assert!(!r.machine_output_enabled && !r.hardware_qualified);
}

#[test]
fn interlock_fault_freezes_at_injection_and_never_completes_the_job() {
    let r = run(FaultKind::EnclosureOpen, 6100);
    assert_eq!(r.outcome, Outcome::Faulted);
    assert_eq!(r.virtual_elapsed_ms, 6100);
    let last = r.frames.last().unwrap();
    assert!(last.position_mm[0] > 1.0 && last.position_mm[0] < 1.1);
    assert!(!last.effective_jet);
    assert_eq!(r.completed_operations, 2);
}

#[test]
fn stalled_host_trips_at_supervisor_boundary_and_stale_inputs_are_distinct() {
    let r = run(FaultKind::WatchdogStall, 6140);
    assert_eq!(r.outcome, Outcome::Faulted);
    assert_eq!(r.virtual_elapsed_ms, 6237); // last successful poll6137 plus independent100ms deadline
    assert!(r.frames.last().unwrap().watchdog_tripped);
    assert!(r.simulated_cut_length_mm < 0.4);
    let stale = run(FaultKind::StaleInput, 6140);
    assert_eq!(stale.outcome, Outcome::Faulted);
    assert!(stale.frames.iter().any(|f| f.state == "Fault(StaleInputs)"));
    assert!(!stale.frames.last().unwrap().watchdog_tripped);
}

#[test]
fn trapped_or_false_release_feedback_is_not_successful_completion() {
    let trapped = run(FaultKind::ReleaseBlocked, 0);
    assert_eq!(trapped.outcome, Outcome::WaitingForRelease);
    assert!(trapped.frames.last().unwrap().pressure_units > 0);
    assert_eq!(trapped.completed_operations, 3);
    let liar = run(FaultKind::PressureSensorLies, 0);
    assert_eq!(liar.outcome, Outcome::UnsafePlant);
    assert!(
        liar.frames
            .iter()
            .any(|f| f.unsafe_recovery && f.pressure_units > 0)
    );
}

#[test]
fn verification_rebuilds_original_and_all_report_fields_and_refuses_unreached_faults() {
    let integration = integration();
    let catalog = cutting::bundled().unwrap();
    let r = run(FaultKind::None, 0);
    let bytes = serde_json::to_vec(&r).unwrap();
    assert_eq!(
        simulation::verify(source(), &integration, &bytes, &catalog).unwrap()["verified"],
        true
    );
    let mut forged = serde_json::to_value(r).unwrap();
    forged["frames"][3]["position_mm"][0] = serde_json::json!(999.0);
    assert!(
        simulation::verify(
            source(),
            &integration,
            &serde_json::to_vec(&forged).unwrap(),
            &catalog
        )
        .is_err()
    );
    forged["machine_output_enabled"] = serde_json::json!(true);
    assert!(
        simulation::verify(
            source(),
            &integration,
            &serde_json::to_vec(&forged).unwrap(),
            &catalog
        )
        .is_err()
    );
    assert!(
        simulation::replay(
            source(),
            &integration,
            &catalog,
            FaultSpec {
                kind: FaultKind::EnclosureOpen,
                at_ms: 1_000_000
            }
        )
        .is_err()
    );
    assert!(simulation::verify(b"different source", &integration, &bytes, &catalog).is_err());
}
