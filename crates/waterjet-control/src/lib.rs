//! Independent simulation policy. No registers, pin mappings, or machine driver.
#![no_std]

#[cfg(feature = "std")]
extern crate std;
#[cfg(feature = "std")]
pub mod simulation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Disarmed,
    Armed,
    Piercing,
    Cutting,
    Depressurizing,
    Fault(Fault),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    Clock,
    StaleInputs,
    UnknownInputs,
    Interlock,
    InvalidTransition,
    Abort,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Poll,
    Arm,
    Pierce,
    Cut,
    Stop,
    Abort,
    Reset,
}

/// Synthetic logical inputs; true means permission, not an electrical polarity.
/// None is unknown and never grants permission. These are proposed requirements,
/// not a verified inventory of WAZER sensors.
#[derive(Clone, Copy, Debug)]
pub struct Inputs {
    pub sampled_at_ms: u64,
    pub emergency_stop_clear: Option<bool>,
    pub enclosure_closed: Option<bool>,
    pub water_ready: Option<bool>,
    pub abrasive_ready: Option<bool>,
    pub motion_ready: Option<bool>,
    pub pressure_released: Option<bool>,
}

/// Logical requests only. De-energizing a request does not prove physical safety.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Requests {
    pub jet: bool,
    pub abrasive: bool,
    pub cutting_motion: bool,
}

pub struct Controller {
    state: State,
    max_input_age_ms: u64,
    last_now: u64,
    last_sample: u64,
}

impl Controller {
    /// Caller supplies the simulation budget; no hardware timing is assumed.
    pub fn new(max_input_age_ms: u64) -> Option<Self> {
        (max_input_age_ms > 0).then_some(Self {
            state: State::Disarmed,
            max_input_age_ms,
            last_now: 0,
            last_sample: 0,
        })
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn requests(&self) -> Requests {
        match self.state {
            State::Piercing | State::Cutting => Requests {
                jet: true,
                abrasive: true,
                cutting_motion: self.state == State::Cutting,
            },
            _ => Requests::default(),
        }
    }

    fn validate(&self, now_ms: u64, inputs: Inputs) -> Result<(), Fault> {
        if now_ms < self.last_now
            || inputs.sampled_at_ms > now_ms
            || inputs.sampled_at_ms < self.last_sample
        {
            return Err(Fault::Clock);
        }
        if now_ms - inputs.sampled_at_ms > self.max_input_age_ms {
            return Err(Fault::StaleInputs);
        }
        let permissions = [
            inputs.emergency_stop_clear,
            inputs.enclosure_closed,
            inputs.water_ready,
            inputs.abrasive_ready,
            inputs.motion_ready,
        ];
        if permissions.contains(&None) || inputs.pressure_released.is_none() {
            return Err(Fault::UnknownInputs);
        }
        if permissions.contains(&Some(false)) {
            return Err(Fault::Interlock);
        }
        Ok(())
    }

    /// Every event checks fresh inputs. Faults latch and never resume a cut.
    /// A future host must poll within its verified budget and independently
    /// disable outputs on host stalls: this pure core cannot detect a stopped CPU.
    pub fn step(&mut self, now_ms: u64, inputs: Inputs, command: Command) -> State {
        let validation = self.validate(now_ms, inputs);
        self.last_now = self.last_now.max(now_ms);
        if inputs.sampled_at_ms <= now_ms {
            self.last_sample = self.last_sample.max(inputs.sampled_at_ms);
        }
        if let State::Fault(_) = self.state {
            if command == Command::Reset
                && validation.is_ok()
                && inputs.pressure_released == Some(true)
            {
                self.state = State::Disarmed;
            }
            return self.state;
        }
        if command == Command::Abort {
            self.state = State::Fault(Fault::Abort);
            return self.state;
        }
        if let Err(fault) = validation {
            self.state = State::Fault(fault);
            return self.state;
        }
        self.state = match (self.state, command) {
            (State::Disarmed, Command::Arm) if inputs.pressure_released == Some(true) => {
                State::Armed
            }
            (State::Armed, Command::Pierce) => State::Piercing,
            (State::Piercing, Command::Cut) => State::Cutting,
            (State::Piercing | State::Cutting, Command::Stop) => State::Depressurizing,
            (State::Disarmed | State::Armed, Command::Stop) => State::Disarmed,
            (State::Depressurizing, Command::Poll | Command::Stop)
                if inputs.pressure_released == Some(true) =>
            {
                State::Disarmed
            }
            (state, Command::Poll) => state,
            (State::Depressurizing, Command::Stop) => State::Depressurizing,
            _ => State::Fault(Fault::InvalidTransition),
        };
        self.state
    }
}
