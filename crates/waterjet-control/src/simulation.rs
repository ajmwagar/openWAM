//! Synthetic virtual-time plant. No WAZER timing, pin, or pressure calibration.
use crate::{Command, Controller, Inputs, Requests, State};
use std::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Model {
    pub input_age_ms: u64,
    pub watchdog_ms: u64,
    pub pressure_capacity: u32,
    pub pressure_rise_per_ms: u32,
    pub pressure_fall_per_ms: u32,
}

pub const SCENARIOS: [&str; 9] = [
    "normal",
    "enclosure-open",
    "watchdog-stall",
    "stale-input",
    "unknown-input",
    "release-blocked",
    "valve-stuck-open",
    "cutoff-stuck-on",
    "pressure-sensor-lies",
];

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Contained,
    HazardDemonstrated,
}

pub struct Report {
    pub scenario: &'static str,
    pub trace: Vec<(&'static str, Snapshot)>,
    pub outcome: Outcome,
}

#[derive(Debug)]
pub enum ScenarioError {
    UnknownScenario,
    Plant(Error),
    UnexpectedOutcome,
}

impl From<Error> for ScenarioError {
    fn from(value: Error) -> Self {
        Self::Plant(value)
    }
}

/// Fixed scripts are reproducible experiments, not free-form orchestration.
pub fn run_scenario(name: &str) -> Result<Report, ScenarioError> {
    let scenario = *SCENARIOS
        .iter()
        .find(|&&s| s == name)
        .ok_or(ScenarioError::UnknownScenario)?;
    let mut sim = Simulator::new(Model::default())?;
    let mut trace = std::vec![
        ("arm", sim.host(Command::Arm)),
        ("pierce", sim.host(Command::Pierce)),
        ("pressure-rise", sim.advance_to(20)?),
        ("cut", sim.host(Command::Cut)),
    ];
    let outcome = match scenario {
        "normal" => {
            trace.push(("cut-progress", sim.advance_to(30)?));
            trace.push(("stop", sim.host(Command::Stop)));
            trace.push(("pressure-release", sim.advance_to(90)?));
            trace.push(("confirm-release", sim.host(Command::Poll)));
            Outcome::Contained
        }
        "enclosure-open" => {
            sim.injection.enclosure_open = true;
            trace.push(("independent-cutoff", sim.advance_to(21)?));
            trace.push(("host-detects", sim.host(Command::Poll)));
            Outcome::Contained
        }
        "watchdog-stall" | "cutoff-stuck-on" => {
            sim.injection.cutoff_stuck_on = scenario == "cutoff-stuck-on";
            trace.push(("host-stalled", sim.advance_to(120)?));
            if scenario == "cutoff-stuck-on" {
                Outcome::HazardDemonstrated
            } else {
                trace.push(("host-restarts", sim.host(Command::Poll)));
                trace.push(("pressure-release", sim.advance_to(220)?));
                trace.push(("reset-refused", sim.host(Command::Reset)));
                Outcome::Contained
            }
        }
        "stale-input" => {
            sim.injection.stale_sample = true;
            trace.push(("sensor-stalled", sim.advance_to(71)?));
            trace.push(("host-detects", sim.host(Command::Poll)));
            Outcome::Contained
        }
        "unknown-input" => {
            sim.injection.unknown_water = true;
            trace.push(("host-detects", sim.host(Command::Poll)));
            Outcome::Contained
        }
        "release-blocked" | "pressure-sensor-lies" => {
            sim.injection.release_blocked = true;
            trace.push(("stop", sim.host(Command::Stop)));
            trace.push(("trapped-pressure", sim.advance_to(90)?));
            sim.injection.pressure_sensor_stuck_released = scenario == "pressure-sensor-lies";
            trace.push(("release-observation", sim.host(Command::Poll)));
            if scenario == "pressure-sensor-lies" {
                Outcome::HazardDemonstrated
            } else {
                Outcome::Contained
            }
        }
        "valve-stuck-open" => {
            sim.injection.jet_valve_stuck_open = true;
            trace.push(("stop", sim.host(Command::Stop)));
            trace.push(("residual-flow", sim.advance_to(21)?));
            trace.push(("pressure-release", sim.advance_to(60)?));
            trace.push(("confirm-release", sim.host(Command::Poll)));
            Outcome::HazardDemonstrated
        }
        _ => return Err(ScenarioError::UnknownScenario),
    };
    let last = trace.last().ok_or(ScenarioError::UnexpectedOutcome)?.1;
    let expected = match scenario {
        "normal" => last.state == State::Disarmed && last.pressure_units == 0,
        "enclosure-open" => {
            last.state == State::Fault(crate::Fault::Interlock) && !last.effective.jet
        }
        "watchdog-stall" => {
            last.watchdog_tripped
                && !last.effective.jet
                && last.pressure_units == 0
                && matches!(last.state, State::Fault(_))
        }
        "stale-input" => {
            last.state == State::Fault(crate::Fault::StaleInputs) && !last.effective.jet
        }
        "unknown-input" => {
            last.state == State::Fault(crate::Fault::UnknownInputs) && !last.effective.jet
        }
        "release-blocked" => {
            last.state == State::Depressurizing && last.pressure_units > 0 && !last.effective.jet
        }
        "cutoff-stuck-on" => last.cutoff_failure && last.pressure_units > 0,
        "pressure-sensor-lies" => last.unsafe_recovery,
        "valve-stuck-open" => {
            trace.iter().any(|(_, s)| s.nozzle_flow && !s.effective.jet) && last.pressure_units == 0
        }
        _ => false,
    };
    if !expected {
        return Err(ScenarioError::UnexpectedOutcome);
    }
    Ok(Report {
        scenario,
        trace,
        outcome,
    })
}

impl Default for Model {
    fn default() -> Self {
        Self {
            input_age_ms: 50,
            watchdog_ms: 100,
            pressure_capacity: 1000,
            pressure_rise_per_ms: 20,
            pressure_fall_per_ms: 10,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Injection {
    pub enclosure_open: bool,
    pub emergency_stop: bool,
    pub unknown_water: bool,
    pub stale_sample: bool,
    pub release_blocked: bool,
    pub jet_valve_stuck_open: bool,
    pub cutoff_stuck_on: bool,
    pub pressure_sensor_stuck_released: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub at_ms: u64,
    pub state: State,
    pub requested: Requests,
    pub effective: Requests,
    pub pressure_units: u32,
    pub nozzle_flow: bool,
    pub watchdog_tripped: bool,
    pub cutoff_failure: bool,
    pub unsafe_recovery: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidModel,
    ClockRegression,
    AdvanceTooLarge,
}

pub struct Simulator {
    controller: Controller,
    model: Model,
    now: u64,
    last_kick: u64,
    last_sample: u64,
    watchdog_tripped: bool,
    pressure: u32,
    pub injection: Injection,
}

impl Simulator {
    pub fn new(model: Model) -> Result<Self, Error> {
        if model.watchdog_ms == 0
            || model.pressure_capacity == 0
            || model.pressure_rise_per_ms == 0
            || model.pressure_fall_per_ms == 0
        {
            return Err(Error::InvalidModel);
        }
        Ok(Self {
            controller: Controller::new(model.input_age_ms).ok_or(Error::InvalidModel)?,
            model,
            now: 0,
            last_kick: 0,
            last_sample: 0,
            watchdog_tripped: false,
            pressure: 0,
            injection: Injection::default(),
        })
    }

    fn effective(&self) -> Requests {
        // Hypothetical independent supply cutoff. Its presence on WAZER is not proven.
        let cutoff =
            self.watchdog_tripped || self.injection.enclosure_open || self.injection.emergency_stop;
        if cutoff && !self.injection.cutoff_stuck_on {
            Requests::default()
        } else {
            self.controller.requests()
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        let effective = self.effective();
        Snapshot {
            at_ms: self.now,
            state: self.controller.state(),
            requested: self.controller.requests(),
            effective,
            pressure_units: self.pressure,
            nozzle_flow: self.pressure > 0
                && (effective.jet || self.injection.jet_valve_stuck_open),
            watchdog_tripped: self.watchdog_tripped,
            cutoff_failure: (self.watchdog_tripped
                || self.injection.enclosure_open
                || self.injection.emergency_stop)
                && effective.jet,
            unsafe_recovery: self.controller.state() == State::Disarmed && self.pressure > 0,
        }
    }

    /// Next independent supervisor boundary, for a host replay scheduler.
    pub fn watchdog_deadline_ms(&self) -> Option<u64> {
        if self.watchdog_tripped {
            None
        } else {
            self.last_kick.checked_add(self.model.watchdog_ms)
        }
    }

    /// Advance the independent plant and watchdog, even when the host is stalled.
    /// 1 ms fixed steps, bounded to 60,000 steps per call. Errors leave state unchanged.
    pub fn advance_to(&mut self, at_ms: u64) -> Result<Snapshot, Error> {
        if at_ms < self.now {
            return Err(Error::ClockRegression);
        }
        if at_ms - self.now > 60_000 {
            return Err(Error::AdvanceTooLarge);
        }
        while self.now < at_ms {
            self.now += 1;
            if self.now - self.last_kick >= self.model.watchdog_ms {
                self.watchdog_tripped = true;
            }
            if self.effective().jet {
                self.pressure = self
                    .pressure
                    .saturating_add(self.model.pressure_rise_per_ms)
                    .min(self.model.pressure_capacity);
            } else if !self.injection.release_blocked {
                self.pressure = self
                    .pressure
                    .saturating_sub(self.model.pressure_fall_per_ms);
            }
        }
        Ok(self.snapshot())
    }

    /// Run one host event. A watchdog trip remains latched; host restart aborts
    /// the core rather than reviving its old cutting request. Constructing a new
    /// simulator is a fresh experiment, not an in-place reset of the watchdog.
    pub fn host(&mut self, command: Command) -> Snapshot {
        if !self.injection.stale_sample {
            self.last_sample = self.now;
        }
        let inputs = Inputs {
            sampled_at_ms: self.last_sample,
            emergency_stop_clear: Some(!self.injection.emergency_stop),
            enclosure_closed: Some(!self.injection.enclosure_open),
            water_ready: if self.injection.unknown_water {
                None
            } else {
                Some(true)
            },
            abrasive_ready: Some(true),
            motion_ready: Some(true),
            pressure_released: Some(
                self.pressure == 0 || self.injection.pressure_sensor_stuck_released,
            ),
        };
        let command = if self.watchdog_tripped {
            Command::Abort
        } else {
            command
        };
        self.controller.step(self.now, inputs, command);
        if !self.watchdog_tripped {
            self.last_kick = self.now;
        }
        self.snapshot()
    }
}
