#![cfg(feature = "std")]
use reversal_waterjet_control::simulation::{
    Error, Model, Outcome, SCENARIOS, Simulator, run_scenario,
};
use reversal_waterjet_control::{Command, Fault, Requests, State};

fn cutting() -> Simulator {
    let mut sim = Simulator::new(Model::default()).unwrap();
    sim.host(Command::Arm);
    sim.host(Command::Pierce);
    sim.advance_to(20).unwrap();
    assert_eq!(sim.host(Command::Cut).state, State::Cutting);
    sim
}

#[test]
fn plant_and_watchdog_run_without_host_events() {
    let mut sim = cutting();
    assert_eq!(sim.watchdog_deadline_ms(), Some(120));
    let before = sim.advance_to(119).unwrap();
    assert!(!before.watchdog_tripped);
    assert_eq!(before.pressure_units, 1000);
    let trip = sim.advance_to(120).unwrap();
    assert!(trip.watchdog_tripped);
    assert_eq!(sim.watchdog_deadline_ms(), None);
    assert!(trip.requested.jet); // stalled core is still requesting a cut
    assert_eq!(trip.effective, Requests::default()); // independent supervisor acts
    assert_eq!(trip.pressure_units, 990); // cutoff does not instantly release pressure
    assert_eq!(sim.host(Command::Reset).state, State::Fault(Fault::Abort));
    assert!(sim.advance_to(220).unwrap().watchdog_tripped);
    assert_eq!(sim.snapshot().pressure_units, 0);
    assert_eq!(sim.host(Command::Arm).effective, Requests::default());
}

#[test]
fn interlock_drops_effective_outputs_before_host_can_observe_it() {
    let mut sim = cutting();
    sim.injection.enclosure_open = true;
    let s = sim.advance_to(21).unwrap();
    assert_eq!(s.state, State::Cutting);
    assert_eq!(s.effective, Requests::default());
    assert!(s.pressure_units > 0);
    assert_eq!(
        sim.host(Command::Poll).state,
        State::Fault(Fault::Interlock)
    );
}

#[test]
fn trapped_pressure_cannot_be_replaced_with_a_fixed_delay() {
    let mut sim = cutting();
    sim.injection.release_blocked = true;
    sim.host(Command::Stop);
    for tick in [90, 180, 270, 360] {
        sim.advance_to(tick).unwrap();
        assert_eq!(sim.host(Command::Poll).state, State::Depressurizing);
        assert_eq!(sim.snapshot().pressure_units, 400);
    }
}

#[test]
fn unsafe_plant_failures_are_exposed_rather_than_reported_as_contained() {
    for name in [
        "valve-stuck-open",
        "cutoff-stuck-on",
        "pressure-sensor-lies",
    ] {
        let r = run_scenario(name).unwrap();
        assert_eq!(r.outcome, Outcome::HazardDemonstrated);
    }
    let liar = run_scenario("pressure-sensor-lies").unwrap();
    let last = liar.trace.last().unwrap().1;
    assert!(last.unsafe_recovery);
    assert!(last.pressure_units > 0);
    assert_eq!(last.state, State::Disarmed);
}

#[test]
fn scripts_are_deterministic_and_unknown_scenarios_fail() {
    for name in SCENARIOS {
        assert_eq!(
            run_scenario(name).unwrap().trace,
            run_scenario(name).unwrap().trace
        );
    }
    assert!(run_scenario("not-a-scenario").is_err());
}

#[test]
fn advancing_in_chunks_does_not_change_the_plant_and_errors_are_atomic() {
    let mut a = cutting();
    let mut b = cutting();
    let expected = a.advance_to(120).unwrap();
    for tick in [21, 30, 99, 119, 120] {
        b.advance_to(tick).unwrap();
    }
    assert_eq!(expected, b.snapshot());
    assert_eq!(b.advance_to(119), Err(Error::ClockRegression));
    assert_eq!(b.advance_to(60_121), Err(Error::AdvanceTooLarge));
    assert_eq!(b.snapshot(), expected);
    assert!(matches!(
        Simulator::new(Model {
            watchdog_ms: 0,
            ..Model::default()
        }),
        Err(Error::InvalidModel)
    ));
}
