use reversal_sketch_intake::{openwam_api as api, shutdown::*};
use serde_json::json;

fn input(events: &[(u64, Event)]) -> Input {
    Input {
        config: Config {
            normal_threshold_seconds: 1.0,
            door_threshold_seconds: 0.1,
            pins: [Pin {
                connected: true,
                inverted: false,
                initial_logical: true,
            }; 6],
            switches: Vec::new(),
        },
        fault: Fault::None,
        events: events
            .iter()
            .map(|(at_us, event)| TimedEvent {
                at_us: *at_us,
                event: *event,
            })
            .collect(),
    }
}
fn writes(replay: &Replay) -> Vec<(u64, Channel, bool)> {
    replay
        .trace
        .iter()
        .filter_map(|v| match v.action {
            Action::Write {
                channel, logical, ..
            } => Some((v.at_us, channel, logical)),
            _ => None,
        })
        .collect()
}

#[test]
fn completion_golden_order_strict_boundary_and_first_poll_epoch() {
    let replay = simulate(&input(&[
        (0, Event::BeginCompletion),
        (50, Event::Poll),
        (1_000_050, Event::Poll),
        (1_000_051, Event::Poll),
    ]))
    .unwrap();
    // Manually specified from the retained 0x20264 write branches, independent of model constants.
    assert_eq!(
        writes(&replay),
        vec![
            (50, Channel::AbrasiveValve, false),
            (1_000_050, Channel::AbrasiveValve, false),
            (1_000_051, Channel::AbrasiveValve, false),
            (1_000_051, Channel::LowPressurePump, false),
            (1_000_051, Channel::AbrasiveMotor, false),
            (1_000_051, Channel::HighPressureValve, false),
            (1_000_051, Channel::HighPressurePump, false),
            (1_000_051, Channel::DumpValve, false)
        ]
    );
    assert!(replay.trace.contains(&TraceEntry {
        at_us: 1_000_050,
        action: Action::TimerWaiting
    }));
    assert_eq!(replay.final_state, State::AllDone);
    assert_eq!(replay.requested_logical, [false; 6]);
}

#[test]
fn door_pause_uses_alternate_threshold_abort_retains_motion_player_order() {
    for (event, threshold) in [
        (Event::PreparePause { reason: 2 }, 100_000),
        (Event::PreparePause { reason: 7 }, 1_000_000),
        (Event::Abort { reason: 11 }, 1_000_000),
    ] {
        let replay = simulate(&input(&[
            (0, event),
            (10, Event::Poll),
            (threshold + 10, Event::Poll),
            (threshold + 11, Event::Poll),
        ]))
        .unwrap();
        assert_eq!(replay.final_state, State::Paused);
        assert_eq!(writes(&replay)[0], (0, Channel::AbrasiveValve, false));
        assert_eq!(writes(&replay)[1].0, threshold + 11);
        assert!(replay.trace.contains(&TraceEntry {
            at_us: threshold + 10,
            action: Action::TimerWaiting
        }));
        if matches!(event, Event::Abort { .. }) {
            assert_eq!(
                replay.trace[..5],
                [
                    TraceEntry {
                        at_us: 0,
                        action: Action::StopMotion
                    },
                    TraceEntry {
                        at_us: 0,
                        action: Action::AbortPlayer
                    },
                    TraceEntry {
                        at_us: 0,
                        action: Action::State {
                            state: State::PrepareToPause
                        },
                    },
                    TraceEntry {
                        at_us: 0,
                        action: Action::FeedHold
                    },
                    TraceEntry {
                        at_us: 0,
                        action: Action::StopModeTimers
                    }
                ]
            );
        }
    }
}

#[test]
fn halt_clear_never_restores_and_ordered_bindings_respect_ignore_connection_inversion() {
    let mut input = input(&[
        (0, Event::SwitchHalt { active: true }),
        (10, Event::SwitchHalt { active: false }),
        (11, Event::Poll),
    ]);
    input.config.pins[0].inverted = true;
    input.config.pins[1].connected = false;
    input.config.switches = vec![
        DigitalSwitch {
            channel: Channel::HighPressurePump,
            halt_logical: true,
            ignore_on_halt: true,
        },
        DigitalSwitch {
            channel: Channel::AbrasiveValve,
            halt_logical: false,
            ignore_on_halt: false,
        },
        DigitalSwitch {
            channel: Channel::LowPressurePump,
            halt_logical: false,
            ignore_on_halt: false,
        },
        DigitalSwitch {
            channel: Channel::AbrasiveMotor,
            halt_logical: false,
            ignore_on_halt: false,
        },
    ];
    let replay = simulate(&input).unwrap();
    assert_eq!(
        writes(&replay),
        vec![
            (0, Channel::AbrasiveValve, false),
            (0, Channel::LowPressurePump, false)
        ]
    );
    assert_eq!(replay.observed_electrical[0], Some(true));
    assert_eq!(replay.observed_electrical[1], None);
    assert!(replay.requested_logical[2]);
    assert_eq!(replay.final_state, State::Running); // No invented waterjet HALT selection.
}

#[test]
fn state_completion_does_not_hide_stuck_or_delayed_output() {
    let mut input = input(&[
        (0, Event::BeginCompletion),
        (0, Event::Poll),
        (1_000_001, Event::Poll),
    ]);
    input.fault = Fault::HoldElectrical {
        channel: Channel::HighPressurePump,
        level: true,
    };
    let replay = simulate(&input).unwrap();
    assert_eq!(replay.final_state, State::AllDone);
    assert_eq!(
        replay.final_output_mismatches,
        vec![Channel::HighPressurePump]
    );
    input.fault = Fault::DelayElectrical {
        channel: Channel::HighPressurePump,
        delay_us: 100,
    };
    let replay = simulate(&input).unwrap();
    assert_eq!(replay.pending_electrical_changes, 1);
    assert_eq!(
        replay.final_output_mismatches,
        vec![Channel::HighPressurePump]
    );
    input.events.push(TimedEvent {
        at_us: 2_000_000,
        event: Event::Poll,
    });
    let replay = simulate(&input).unwrap();
    assert!(replay.trace.contains(&TraceEntry {
        at_us: 1_000_101,
        action: Action::ElectricalObserved {
            channel: Channel::HighPressurePump,
            level: false
        }
    }));
    assert!(replay.final_output_mismatches.is_empty());
    assert_eq!(replay.pending_electrical_changes, 0);
}

#[test]
fn replay_is_deterministic_verification_rejects_trace_config_evidence_and_flag_tampering() {
    let input = input(&[
        (0, Event::PreparePause { reason: 2 }),
        (0, Event::Poll),
        (100_001, Event::Poll),
    ]);
    let replay = serde_json::to_value(simulate(&input).unwrap()).unwrap();
    assert_eq!(
        replay,
        serde_json::to_value(simulate(&input).unwrap()).unwrap()
    );
    assert_eq!(verify(&input, &replay).unwrap()["verified"], true);
    for pointer in [
        "/trace/0/at_us",
        "/evidence/firmware_sha256",
        "/machine_output_enabled",
        "/input/config/normal_threshold_seconds",
    ] {
        let mut forged = replay.clone();
        *forged.pointer_mut(pointer).unwrap() = json!(999);
        assert!(verify(&input, &forged).is_err());
    }
    let mut other = input.clone();
    other.config.door_threshold_seconds = 0.2;
    assert!(verify(&other, &replay).is_err());
    let trace: Vec<TraceEntry> = serde_json::from_value(replay["trace"].clone()).unwrap();
    assert_eq!(compare(&input, &trace).unwrap()["matches"], true);
    let mut changed = trace;
    changed[0].at_us = 1;
    assert_eq!(compare(&input, &changed).unwrap()["difference_count"], 1);
}

#[test]
fn invalid_clocks_budgets_thresholds_and_transitions_fail_explicitly() {
    let mut input = input(&[(0, Event::BeginCompletion)]);
    input.config.normal_threshold_seconds = f32::NAN;
    assert!(simulate(&input).is_err());
    input.config.normal_threshold_seconds = 1.0;
    input.events.push(TimedEvent {
        at_us: 0,
        event: Event::BeginCompletion,
    });
    assert!(simulate(&input).is_err());
    input.events = vec![
        TimedEvent {
            at_us: 0,
            event: Event::BeginCompletion,
        },
        TimedEvent {
            at_us: 0,
            event: Event::Poll,
        },
        TimedEvent {
            at_us: 1,
            event: Event::Abort { reason: 2 },
        },
    ];
    assert!(simulate(&input).is_err());
    input.events = vec![
        TimedEvent {
            at_us: 2,
            event: Event::Poll,
        },
        TimedEvent {
            at_us: 1,
            event: Event::Poll,
        },
    ];
    assert!(simulate(&input).is_err());
    input.events = vec![TimedEvent {
        at_us: MAX_TIME_US + 1,
        event: Event::Poll,
    }];
    assert!(simulate(&input).is_err());
    input.events = vec![
        TimedEvent {
            at_us: 0,
            event: Event::Poll
        };
        MAX_EVENTS + 1
    ];
    assert!(simulate(&input).is_err());
}

#[test]
fn api_schema_and_cli_replay_verification_roundtrip() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let input = input(&[
        (0, Event::BeginCompletion),
        (0, Event::Poll),
        (1_000_001, Event::Poll),
    ]);
    let invoke = |operation| {
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
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(output.stderr.is_empty());
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    let replayed = invoke(json!({"operation":"replay_shutdown","input":input}));
    assert_eq!(replayed["result"]["replay"]["final_state"], "all_done");
    let verified = invoke(
        json!({"operation":"verify_shutdown_replay","input":input,"replay":replayed["result"]["replay"]}),
    );
    assert_eq!(verified["result"]["verification"]["verified"], true);
    assert!(
        api::capabilities()["request_schema"]
            .to_string()
            .contains("compare_shutdown_trace")
    );
}

#[test]
fn ordered_halt_then_abort_clear_keeps_pause_timer_running() {
    let mut input = input(&[
        (0, Event::SwitchHalt { active: true }),
        (0, Event::Abort { reason: 11 }),
        (0, Event::Poll),
        (500_000, Event::SwitchHalt { active: false }),
        (1_000_001, Event::Poll),
    ]);
    input.config.switches = Channel::ALL
        .into_iter()
        .map(|channel| DigitalSwitch {
            channel,
            halt_logical: false,
            ignore_on_halt: false,
        })
        .collect();
    let replay = simulate(&input).unwrap();
    let actual = writes(&replay);
    assert_eq!(
        actual[..6],
        [
            (0, Channel::LowPressurePump, false),
            (0, Channel::AbrasiveMotor, false),
            (0, Channel::HighPressurePump, false),
            (0, Channel::HighPressureValve, false),
            (0, Channel::AbrasiveValve, false),
            (0, Channel::DumpValve, false)
        ]
    );
    let stop_motion = replay
        .trace
        .iter()
        .position(|entry| entry.action == Action::StopMotion)
        .unwrap();
    let last_switch = replay
        .trace
        .iter()
        .position(|entry| {
            entry.action
                == Action::Write {
                    channel: Channel::DumpValve,
                    logical: false,
                    electrical: false,
                }
        })
        .unwrap();
    assert!(last_switch < stop_motion);
    assert_eq!(replay.final_state, State::Paused);
    assert!(!actual.iter().any(|(at, _, _)| *at == 500_000));
}

#[test]
fn zero_threshold_requires_later_poll_and_zero_delay_is_immediate() {
    let mut input = input(&[
        (0, Event::BeginCompletion),
        (0, Event::Poll),
        (0, Event::Poll),
        (1, Event::Poll),
    ]);
    input.config.normal_threshold_seconds = 0.0;
    input.fault = Fault::DelayElectrical {
        channel: Channel::HighPressurePump,
        delay_us: 0,
    };
    let replay = simulate(&input).unwrap();
    assert_eq!(replay.final_state, State::AllDone);
    assert_eq!(replay.pending_electrical_changes, 0);
    assert!(replay.final_output_mismatches.is_empty());
    assert!(replay.trace.contains(&TraceEntry {
        at_us: 0,
        action: Action::TimerWaiting
    }));
}
