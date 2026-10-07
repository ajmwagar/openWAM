//! Independent offline model of the traced shutdown subset, not firmware execution.
use anyhow::{Result, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const MAX_EVENTS: usize = 1024;
pub const MAX_TIME_US: u64 = 60_000_000;
pub const MAX_SWITCHES: usize = 16;
const SHUTDOWN_EVIDENCE: &str = include_str!("../../../wazer/shutdown-research.json");
const SWITCH_EVIDENCE: &str = include_str!("../../../wazer/switch-halt-research.json");
const TIMER_EVIDENCE: &str = include_str!("../../../wazer/shutdown-timer-boundary.json");

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    LowPressurePump,
    AbrasiveMotor,
    HighPressurePump,
    HighPressureValve,
    AbrasiveValve,
    DumpValve,
}
impl Channel {
    pub const ALL: [Self; 6] = [
        Self::LowPressurePump,
        Self::AbrasiveMotor,
        Self::HighPressurePump,
        Self::HighPressureValve,
        Self::AbrasiveValve,
        Self::DumpValve,
    ];
    fn index(self) -> usize {
        Self::ALL.iter().position(|v| *v == self).unwrap()
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    pub connected: bool,
    pub inverted: bool,
    pub initial_logical: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DigitalSwitch {
    pub channel: Channel,
    pub halt_logical: bool,
    pub ignore_on_halt: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Explicit caller values. Constructor defaults are examples, not installed values.
    pub normal_threshold_seconds: f32,
    pub door_threshold_seconds: f32,
    /// Channel::ALL order; no physical port/connector assignments are implied.
    pub pins: [Pin; 6],
    /// Explicit dispatch order. Configuration parsing/precedence is outside this model.
    pub switches: Vec<DigitalSwitch>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Fault {
    None,
    HoldElectrical { channel: Channel, level: bool },
    DelayElectrical { channel: Channel, delay_us: u64 },
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    BeginCompletion,
    PreparePause {
        reason: u8,
    },
    Abort {
        reason: u8,
    },
    /// Only the established Switch callback path. Waterjet guard resolution is separate.
    SwitchHalt {
        active: bool,
    },
    Poll,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TimedEvent {
    pub at_us: u64,
    pub event: Event,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub config: Config,
    pub fault: Fault,
    pub events: Vec<TimedEvent>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Running,
    CompletedCut,
    PrepareToPause,
    Pausing,
    AllDone,
    Paused,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    State {
        state: State,
    },
    FeedHold,
    StopModeTimers,
    StopMotion,
    AbortPlayer,
    TimerStarted,
    TimerStopped,
    TimerWaiting,
    Write {
        channel: Channel,
        logical: bool,
        electrical: bool,
    },
    ElectricalObserved {
        channel: Channel,
        level: bool,
    },
    SwitchHalt {
        active: bool,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TraceEntry {
    pub at_us: u64,
    pub action: Action,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct Evidence {
    pub firmware_sha256: String,
    pub shutdown_receipt_sha256: String,
    pub switch_receipt_sha256: String,
    pub timer_receipt_sha256: String,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct Replay {
    pub schema: &'static str,
    pub input_sha256: String,
    pub evidence: Evidence,
    pub input: Input,
    pub trace: Vec<TraceEntry>,
    pub final_state: State,
    pub requested_logical: [bool; 6],
    pub observed_electrical: [Option<bool>; 6],
    pub final_output_mismatches: Vec<Channel>,
    pub pending_electrical_changes: usize,
    pub firmware_executed: bool,
    pub machine_output_enabled: bool,
    pub controller_qualified: bool,
    pub hardware_qualified: bool,
    pub limitations: Vec<&'static str>,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn evidence() -> Result<Evidence> {
    let receipt: serde_json::Value = serde_json::from_str(SHUTDOWN_EVIDENCE)?;
    Ok(Evidence {
        firmware_sha256: receipt["firmware"]["sha256"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing pinned firmware identity"))?
            .into(),
        shutdown_receipt_sha256: digest(SHUTDOWN_EVIDENCE.as_bytes()),
        switch_receipt_sha256: digest(SWITCH_EVIDENCE.as_bytes()),
        timer_receipt_sha256: digest(TIMER_EVIDENCE.as_bytes()),
    })
}

struct Model<'a> {
    config: &'a Config,
    fault: Fault,
    state: State,
    reason: u8,
    timer: Option<u64>,
    requested: [bool; 6],
    electrical: [Option<bool>; 6],
    pending: BTreeMap<(u64, usize), (Channel, bool)>,
    trace: Vec<TraceEntry>,
}
impl Model<'_> {
    fn record(&mut self, at_us: u64, action: Action) {
        self.trace.push(TraceEntry { at_us, action });
    }
    fn state(&mut self, at_us: u64, state: State) {
        self.state = state;
        self.record(at_us, Action::State { state });
    }
    fn observe(&mut self, at_us: u64, channel: Channel, level: bool) {
        if self.electrical[channel.index()] != Some(level) {
            self.electrical[channel.index()] = Some(level);
            self.record(at_us, Action::ElectricalObserved { channel, level });
        }
    }
    fn advance(&mut self, at_us: u64) {
        while self
            .pending
            .first_key_value()
            .is_some_and(|(key, _)| key.0 <= at_us)
        {
            let ((due, _), (channel, level)) = self.pending.pop_first().unwrap();
            self.observe(due, channel, level);
        }
    }
    fn write(&mut self, at_us: u64, channel: Channel, logical: bool) {
        let pin = self.config.pins[channel.index()];
        if !pin.connected {
            return;
        }
        self.requested[channel.index()] = logical;
        let electrical = logical ^ pin.inverted;
        self.record(
            at_us,
            Action::Write {
                channel,
                logical,
                electrical,
            },
        );
        match self.fault {
            Fault::HoldElectrical { channel: held, .. } if held == channel => {}
            Fault::DelayElectrical {
                channel: delayed,
                delay_us,
            } if delayed == channel && delay_us > 0 => {
                // Each write propagates independently; equal-time ties retain source order.
                self.pending
                    .insert((at_us + delay_us, self.trace.len()), (channel, electrical));
            }
            _ => self.observe(at_us, channel, electrical),
        }
    }
    fn prepare_pause(&mut self, at_us: u64, reason: u8) {
        self.reason = reason;
        self.timer = None;
        self.state(at_us, State::PrepareToPause);
        self.record(at_us, Action::FeedHold);
        self.record(at_us, Action::StopModeTimers);
        self.write(at_us, Channel::AbrasiveValve, false);
        self.state(at_us, State::Pausing);
    }
    fn poll(&mut self, at_us: u64) {
        if !matches!(self.state, State::CompletedCut | State::Pausing) {
            return;
        }
        if self.state == State::CompletedCut {
            self.write(at_us, Channel::AbrasiveValve, false);
        }
        let Some(start) = self.timer else {
            self.timer = Some(at_us);
            self.record(at_us, Action::TimerStarted);
            return;
        };
        let threshold = if self.state == State::Pausing && self.reason == 2 {
            self.config.door_threshold_seconds
        } else {
            self.config.normal_threshold_seconds
        };
        // Firmware timer converts an integer interval to f32 and divides by 1e6.
        // Its swapped float comparator (0x41cf0) tests threshold < elapsed.
        let elapsed_seconds = (at_us - start) as f32 / 1_000_000.0;
        if elapsed_seconds <= threshold {
            self.record(at_us, Action::TimerWaiting);
            return;
        }
        self.record(at_us, Action::TimerStopped);
        for channel in [
            Channel::LowPressurePump,
            Channel::AbrasiveMotor,
            Channel::HighPressureValve,
            Channel::HighPressurePump,
            Channel::DumpValve,
        ] {
            self.write(at_us, channel, false);
        }
        self.timer = None;
        self.state(
            at_us,
            if self.state == State::CompletedCut {
                State::AllDone
            } else {
                State::Paused
            },
        );
    }
}

pub fn simulate(input: &Input) -> Result<Replay> {
    ensure!(
        !input.events.is_empty() && input.events.len() <= MAX_EVENTS,
        "expected 1..={MAX_EVENTS} events"
    );
    ensure!(
        input.config.switches.len() <= MAX_SWITCHES,
        "at most {MAX_SWITCHES} digital Switch bindings"
    );
    for threshold in [
        input.config.normal_threshold_seconds,
        input.config.door_threshold_seconds,
    ] {
        ensure!(
            threshold.is_finite() && (0.0..=60.0).contains(&threshold),
            "threshold must be finite and within 0..=60 seconds"
        );
    }
    ensure!(
        input.events.iter().all(|v| v.at_us <= MAX_TIME_US),
        "replay exceeds 60 seconds"
    );
    ensure!(
        input.events.windows(2).all(|v| v[0].at_us <= v[1].at_us),
        "event clock regressed"
    );
    if let Fault::DelayElectrical { delay_us, .. } = input.fault {
        ensure!(delay_us <= MAX_TIME_US, "output delay exceeds 60 seconds");
    }
    let mut model = Model {
        config: &input.config,
        fault: input.fault,
        state: State::Running,
        reason: 0,
        timer: None,
        requested: input.config.pins.map(|pin| pin.initial_logical),
        electrical: input
            .config
            .pins
            .map(|pin| pin.connected.then_some(pin.initial_logical ^ pin.inverted)),
        pending: BTreeMap::new(),
        trace: Vec::new(),
    };
    if let Fault::HoldElectrical { channel, level } = input.fault {
        model.electrical[channel.index()] = input.config.pins[channel.index()]
            .connected
            .then_some(level);
    }
    for timed in &input.events {
        model.advance(timed.at_us);
        match timed.event {
            Event::BeginCompletion => {
                ensure!(
                    model.state == State::Running,
                    "completion requires running state"
                );
                model.timer = None;
                model.state(timed.at_us, State::CompletedCut);
            }
            Event::PreparePause { reason } | Event::Abort { reason } => {
                ensure!(
                    model.state == State::Running,
                    "pause/abort requires running state; completion-to-pause timer reuse is unresolved"
                );
                if matches!(timed.event, Event::Abort { .. }) {
                    model.record(timed.at_us, Action::StopMotion);
                    model.record(timed.at_us, Action::AbortPlayer);
                }
                model.prepare_pause(timed.at_us, reason);
            }
            Event::SwitchHalt { active } => {
                model.record(timed.at_us, Action::SwitchHalt { active });
                if active {
                    for switch in &input.config.switches {
                        if !switch.ignore_on_halt {
                            model.write(timed.at_us, switch.channel, switch.halt_logical);
                        }
                    }
                }
            }
            Event::Poll => model.poll(timed.at_us),
        }
    }
    let mismatches = Channel::ALL
        .into_iter()
        .filter(|channel| {
            let i = channel.index();
            let pin = input.config.pins[i];
            pin.connected && model.electrical[i] != Some(model.requested[i] ^ pin.inverted)
        })
        .collect();
    Ok(Replay {
        schema: "openwam.shutdown-replay.v1",
        input_sha256: digest(&serde_json::to_vec(input)?),
        evidence: evidence()?,
        input: input.clone(),
        trace: model.trace,
        final_state: model.state,
        requested_logical: model.requested,
        observed_electrical: model.electrical,
        final_output_mismatches: mismatches,
        pending_electrical_changes: model.pending.len(),
        firmware_executed: false,
        machine_output_enabled: false,
        controller_qualified: false,
        hardware_qualified: false,
        limitations: vec![
            "Independent shutdown-handler subset; explicit configuration and poll schedule, not installed state.",
            "No mode application, G-code execution, runtime config precedence or debugger execution.",
            "Digital Switch HALT only; waterjet HALT guard/state selection must be supplied as a separate resolved abort event.",
            "Timer epoch/reset assumed fresh at entry; bounded integer microseconds with f32 seconds, no wraparound or IRQ timing.",
            "Feed hold initially clear; feed-hold/player bookkeeping outside the traced output sequence is abstracted.",
            "Electrical observations are synthetic; no pressure, valve position or hydraulic release inferred.",
        ],
    })
}

pub fn verify(input: &Input, retained: &serde_json::Value) -> Result<serde_json::Value> {
    ensure!(
        serde_json::to_vec(retained)?.len() <= 16 * 1024 * 1024,
        "shutdown replay exceeds 16 MiB"
    );
    let replay = simulate(input)?;
    ensure!(
        crate::json_identity::equivalent(retained, &serde_json::to_value(&replay)?),
        "shutdown replay differs from recomputed input/evidence/trace"
    );
    Ok(
        serde_json::json!({"schema":"openwam.shutdown-verification.v1", "verified":true,
        "input_sha256":replay.input_sha256, "final_state":replay.final_state, "firmware_executed":false}),
    )
}

/// Exact comparison with a caller-supplied trace, not a claim of vendor execution.
pub fn compare(input: &Input, reference: &[TraceEntry]) -> Result<serde_json::Value> {
    ensure!(
        reference.len() <= 100_000,
        "reference trace exceeds entry budget"
    );
    let replay = simulate(input)?;
    let differences: Vec<_> = (0..reference.len().max(replay.trace.len()))
        .filter(|i| reference.get(*i) != replay.trace.get(*i))
        .collect();
    let details: Vec<_> = differences
        .iter()
        .take(32)
        .map(|i| {
            serde_json::json!({"index":i,
        "reference":reference.get(*i), "modeled":replay.trace.get(*i)})
        })
        .collect();
    Ok(
        serde_json::json!({"schema":"openwam.shutdown-comparison.v1", "matches":differences.is_empty(),
        "difference_count":differences.len(), "details":details, "input_sha256":replay.input_sha256,
        "reference_trace_sha256":digest(&serde_json::to_vec(reference)?),
        "reference_basis":"caller_supplied", "firmware_executed":false}),
    )
}
