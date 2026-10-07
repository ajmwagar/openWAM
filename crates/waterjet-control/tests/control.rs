use reversal_waterjet_control::{Command, Controller, Fault, Inputs, Requests, State};

fn inputs(at: u64, released: bool) -> Inputs {
    Inputs {
        sampled_at_ms: at,
        emergency_stop_clear: Some(true),
        enclosure_closed: Some(true),
        water_ready: Some(true),
        abrasive_ready: Some(true),
        motion_ready: Some(true),
        pressure_released: Some(released),
    }
}

fn cutting() -> Controller {
    let mut c = Controller::new(50).unwrap();
    c.step(0, inputs(0, true), Command::Arm);
    c.step(1, inputs(1, false), Command::Pierce);
    assert_eq!(c.step(2, inputs(2, false), Command::Cut), State::Cutting);
    c
}

#[test]
fn normal_sequence_waits_for_pressure_feedback() {
    let mut c = cutting();
    assert_eq!(
        c.requests(),
        Requests {
            jet: true,
            abrasive: true,
            cutting_motion: true
        }
    );
    assert_eq!(
        c.step(3, inputs(3, false), Command::Stop),
        State::Depressurizing
    );
    assert_eq!(c.requests(), Requests::default());
    assert_eq!(
        c.step(1000, inputs(1000, false), Command::Poll),
        State::Depressurizing
    );
    assert_eq!(
        c.step(1001, inputs(1001, true), Command::Poll),
        State::Disarmed
    );
}

#[test]
fn every_missing_or_failed_permission_latches_off() {
    for field in 0..6 {
        for value in [None, Some(false)] {
            // False pressure-release is normal while cutting, not a failed permission.
            if field == 5 && value == Some(false) {
                continue;
            }
            let mut c = cutting();
            let mut i = inputs(3, false);
            match field {
                0 => i.emergency_stop_clear = value,
                1 => i.enclosure_closed = value,
                2 => i.water_ready = value,
                3 => i.abrasive_ready = value,
                4 => i.motion_ready = value,
                _ => i.pressure_released = value,
            }
            assert!(matches!(c.step(3, i, Command::Poll), State::Fault(_)));
            assert_eq!(c.requests(), Requests::default());
            assert!(matches!(
                c.step(4, inputs(4, true), Command::Cut),
                State::Fault(_)
            ));
            assert_eq!(c.requests(), Requests::default());
        }
    }
}

#[test]
fn stale_regressed_and_future_observations_fail() {
    for (now, at, expected) in [
        (53, 2, Fault::StaleInputs),
        (1, 1, Fault::Clock),
        (3, 4, Fault::Clock),
        (3, 1, Fault::Clock),
    ] {
        let mut c = cutting();
        assert_eq!(
            c.step(now, inputs(at, false), Command::Poll),
            State::Fault(expected)
        );
        assert_eq!(c.requests(), Requests::default());
    }
    let mut c = cutting();
    assert_eq!(c.step(52, inputs(2, false), Command::Poll), State::Cutting);
}

#[test]
fn recovery_requires_fresh_clear_inputs_and_pressure_release_then_rearm() {
    let mut c = cutting();
    assert_eq!(
        c.step(3, inputs(3, false), Command::Abort),
        State::Fault(Fault::Abort)
    );
    assert_eq!(
        c.step(4, inputs(4, false), Command::Reset),
        State::Fault(Fault::Abort)
    );
    assert_eq!(
        c.step(55, inputs(4, true), Command::Reset),
        State::Fault(Fault::Abort)
    );
    assert_eq!(
        c.step(56, inputs(56, true), Command::Reset),
        State::Disarmed
    );
    assert_eq!(c.requests(), Requests::default());
    assert_eq!(
        c.step(57, inputs(57, true), Command::Cut),
        State::Fault(Fault::InvalidTransition)
    );
}

#[test]
fn all_short_command_sequences_obey_request_invariants() {
    let commands = [
        Command::Poll,
        Command::Arm,
        Command::Pierce,
        Command::Cut,
        Command::Stop,
        Command::Abort,
        Command::Reset,
    ];
    // Exhaust all 7^5 sequences across both pressure feedback states.
    for released in [false, true] {
        for code in 0..7usize.pow(5) {
            let mut c = Controller::new(50).unwrap();
            let mut remaining = code;
            for tick in 0..5 {
                let command = commands[remaining % 7];
                remaining /= 7;
                let before = c.state();
                let state = c.step(tick, inputs(tick, released), command);
                let requests = c.requests();
                assert!(!requests.cutting_motion || state == State::Cutting);
                assert_eq!(
                    requests.jet,
                    matches!(state, State::Piercing | State::Cutting)
                );
                assert_eq!(requests.abrasive, requests.jet);
                if matches!(before, State::Fault(_)) {
                    assert!(matches!(state, State::Fault(_) | State::Disarmed));
                    assert_eq!(requests, Requests::default());
                }
            }
        }
    }
}

#[test]
fn unsafe_initial_pressure_and_zero_budget_never_arm() {
    assert!(Controller::new(0).is_none());
    let mut c = Controller::new(50).unwrap();
    assert_eq!(
        c.step(0, inputs(0, false), Command::Arm),
        State::Fault(Fault::InvalidTransition)
    );
    assert_eq!(c.requests(), Requests::default());
}
