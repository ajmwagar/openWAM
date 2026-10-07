//! Verified owner operations -> synthetic controller/plant replay. No machine I/O.
use crate::{bridge, cutting::Catalog};
use anyhow::{Result, ensure};
use reversal_waterjet_control::{
    Command, State,
    simulation::{Model, Simulator, Snapshot},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use swarf_waterjet::Operation;

const MAX_TIME_MS: u64 = 3_600_000;
const MAX_OPERATIONS: usize = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FaultKind {
    None,
    EnclosureOpen,
    WatchdogStall,
    StaleInput,
    ReleaseBlocked,
    PressureSensorLies,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FaultSpec {
    pub kind: FaultKind,
    pub at_ms: u64,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Frame {
    pub operation: usize,
    pub phase: &'static str,
    pub at_ms: u64,
    pub state: String,
    pub position_mm: [f64; 2],
    pub requested_jet: bool,
    pub effective_jet: bool,
    pub pressure_units: u32,
    pub nozzle_flow: bool,
    pub watchdog_tripped: bool,
    pub unsafe_recovery: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Completed,
    Faulted,
    WaitingForRelease,
    UnsafePlant,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Replay {
    pub schema: &'static str,
    pub source_sha256: String,
    pub integration_sha256: String,
    pub tables_canonical_sha256: String,
    pub fault: FaultSpec,
    pub model: serde_json::Value,
    pub operation_count: usize,
    pub completed_operations: usize,
    pub virtual_elapsed_ms: u64,
    pub simulated_cut_length_mm: f64,
    pub outcome: Outcome,
    pub frames: Vec<Frame>,
    pub machine_output_enabled: bool,
    pub hardware_qualified: bool,
    pub limitations: Vec<&'static str>,
}

struct Runner {
    sim: Simulator,
    fault: FaultSpec,
    injected: bool,
    position: [f64; 2],
    cut_length: f64,
    frames: Vec<Frame>,
}
impl Runner {
    fn frame(&mut self, op: usize, phase: &'static str) {
        let s = self.sim.snapshot();
        self.frames.push(Frame {
            operation: op,
            phase,
            at_ms: s.at_ms,
            state: format!("{:?}", s.state),
            position_mm: self.position,
            requested_jet: s.requested.jet,
            effective_jet: s.effective.jet,
            pressure_units: s.pressure_units,
            nozzle_flow: s.nozzle_flow,
            watchdog_tripped: s.watchdog_tripped,
            unsafe_recovery: s.unsafe_recovery,
        });
    }
    fn inject(&mut self, op: usize) {
        if self.fault.kind == FaultKind::None
            || self.injected
            || self.sim.snapshot().at_ms < self.fault.at_ms
        {
            return;
        }
        self.injected = true;
        match self.fault.kind {
            FaultKind::EnclosureOpen => self.sim.injection.enclosure_open = true,
            FaultKind::StaleInput => self.sim.injection.stale_sample = true,
            FaultKind::ReleaseBlocked => self.sim.injection.release_blocked = true,
            FaultKind::PressureSensorLies => {
                self.sim.injection.release_blocked = true;
                self.sim.injection.pressure_sensor_stuck_released = true;
            }
            FaultKind::WatchdogStall | FaultKind::None => (),
        }
        self.frame(op, "fault_injected");
    }
    fn host(&mut self, command: Command) -> Snapshot {
        if self.injected && self.fault.kind == FaultKind::WatchdogStall {
            self.sim.snapshot()
        } else {
            self.sim.host(command)
        }
    }
    fn stopped(&self) -> Option<Outcome> {
        let s = self.sim.snapshot();
        if s.unsafe_recovery || s.cutoff_failure {
            Some(Outcome::UnsafePlant)
        } else if s.watchdog_tripped || matches!(s.state, State::Fault(_)) {
            Some(Outcome::Faulted)
        } else {
            None
        }
    }
    fn wait(
        &mut self,
        op: usize,
        duration: u64,
        target: Option<[f64; 2]>,
        cutting: bool,
    ) -> Result<Option<Outcome>> {
        let start = self.sim.snapshot().at_ms;
        let end = start
            .checked_add(duration)
            .ok_or_else(|| anyhow::anyhow!("time overflow"))?;
        ensure!(
            end <= MAX_TIME_MS,
            "simulation exceeds one-hour virtual-time budget"
        );
        let origin = self.position;
        let distance = target
            .map(|to| (to[0] - origin[0]).hypot(to[1] - origin[1]))
            .unwrap_or(0.0);
        let mut previous_fraction = 0.0;
        while self.sim.snapshot().at_ms < end {
            self.inject(op);
            self.host(Command::Poll);
            if let Some(outcome) = self.stopped() {
                self.frame(op, "interrupted");
                return Ok(Some(outcome));
            }
            let now = self.sim.snapshot().at_ms;
            let mut next = (now + 20).min(end);
            if let Some(deadline) = self.sim.watchdog_deadline_ms() {
                next = next.min(deadline);
            }
            if !self.injected && self.fault.kind != FaultKind::None && self.fault.at_ms > now {
                next = next.min(self.fault.at_ms);
            }
            self.sim
                .advance_to(next)
                .map_err(|e| anyhow::anyhow!("plant error: {e:?}"))?;
            let fraction = (next - start) as f64 / duration as f64;
            // Host-side kinematic preview only; no acceleration or step generation.
            if let Some(to) = target {
                self.position = [
                    origin[0] + (to[0] - origin[0]) * fraction,
                    origin[1] + (to[1] - origin[1]) * fraction,
                ];
                if cutting {
                    self.cut_length += distance * (fraction - previous_fraction);
                }
            }
            previous_fraction = fraction;
            self.inject(op);
            self.host(Command::Poll);
            if let Some(outcome) = self.stopped() {
                self.frame(op, "detected");
                self.sim.host(Command::Abort);
                self.frame(op, "interrupted");
                return Ok(Some(outcome));
            }
        }
        if let Some(to) = target {
            self.position = to;
        }
        Ok(None)
    }
}

fn millis(seconds: f64) -> Result<u64> {
    let value = (seconds * 1000.0).ceil();
    ensure!(
        value.is_finite() && value > 0.0 && value <= MAX_TIME_MS as f64,
        "invalid or oversized operation duration"
    );
    Ok(value as u64)
}

pub fn replay(
    original: &[u8],
    integration: &[u8],
    catalog: &Catalog,
    fault: FaultSpec,
) -> Result<Replay> {
    let job = bridge::verified_integration(original, integration, catalog)?;
    replay_draft(
        &job.swarf,
        format!("{:x}", Sha256::digest(original)),
        format!("{:x}", Sha256::digest(integration)),
        job.planning.tables_canonical_sha256,
        fault,
        "Straight-line kinematic preview at nominal table feed; no acceleration, step generation, kerf, leads, tabs or qualified postprocessor.",
    )
}

/// Internal runner for drafts already independently verified by the source adapter.
pub(crate) fn replay_draft(
    draft: &swarf_waterjet::Draft,
    source_sha256: String,
    integration_sha256: String,
    tables_canonical_sha256: String,
    fault: FaultSpec,
    motion_scope: &'static str,
) -> Result<Replay> {
    ensure!(
        fault.at_ms <= MAX_TIME_MS && (fault.kind != FaultKind::None || fault.at_ms == 0),
        "invalid fault schedule"
    );
    ensure!(
        draft.operations.len() <= MAX_OPERATIONS,
        "simulation operation bound exceeded"
    );
    let model = Model::default();
    let mut r = Runner {
        sim: Simulator::new(model).map_err(|e| anyhow::anyhow!("{e:?}"))?,
        fault,
        injected: false,
        position: [0.0, 0.0],
        cut_length: 0.0,
        frames: Vec::new(),
    };
    let mut completed = 0;
    let mut outcome = Outcome::Completed;
    for (i, operation) in draft.operations.iter().enumerate() {
        r.inject(i);
        r.host(Command::Poll);
        r.frame(i, "start");
        if let Some(stop) = r.stopped() {
            outcome = stop;
            r.frame(i, "interrupted");
            break;
        }
        let stopped = match *operation {
            Operation::Rapid { to_mm } => {
                ensure!(
                    r.sim.snapshot().state == State::Disarmed,
                    "rapid requires disarmed controller"
                );
                let distance = (to_mm[0] - r.position[0]).hypot(to_mm[1] - r.position[1]);
                if distance == 0.0 {
                    r.position = to_mm;
                    None
                } else {
                    r.wait(
                        i,
                        millis(distance / draft.request.max_feed_mm_min * 60.0)?,
                        Some(to_mm),
                        false,
                    )?
                }
            }
            Operation::Pierce { seconds } => {
                r.host(Command::Arm);
                r.host(Command::Pierce);
                if let Some(stop) = r.stopped() {
                    Some(stop)
                } else {
                    r.wait(i, millis(seconds)?, None, false)?
                }
            }
            Operation::Cut { to_mm, feed_mm_min } => {
                if r.sim.snapshot().state == State::Piercing {
                    r.host(Command::Cut);
                }
                let distance = (to_mm[0] - r.position[0]).hypot(to_mm[1] - r.position[1]);
                if let Some(stop) = r.stopped() {
                    Some(stop)
                } else {
                    r.wait(i, millis(distance / feed_mm_min * 60.0)?, Some(to_mm), true)?
                }
            }
            Operation::Stop => {
                r.host(Command::Stop);
                let mut stop = r.stopped();
                // Synthetic release observation budget. Never accept timeout as release.
                for _ in 0..50 {
                    if stop.is_some() || r.sim.snapshot().state == State::Disarmed {
                        break;
                    }
                    stop = r.wait(i, 20, None, false)?;
                }
                stop.or_else(|| {
                    (r.sim.snapshot().state != State::Disarmed)
                        .then_some(Outcome::WaitingForRelease)
                })
            }
        };
        r.frame(i, "end");
        if let Some(stop) = stopped {
            outcome = stop;
            break;
        }
        completed += 1;
    }
    ensure!(
        fault.kind == FaultKind::None || r.injected,
        "fault time falls after completed job"
    );
    Ok(Replay {
        schema: "reversal.waterjet-job-simulation.v1",
        source_sha256,
        integration_sha256,
        tables_canonical_sha256,
        fault,
        model: serde_json::json!({"input_age_ms":model.input_age_ms,"watchdog_ms":model.watchdog_ms,"pressure_capacity_units":model.pressure_capacity,"pressure_rise_per_ms":model.pressure_rise_per_ms,"pressure_fall_per_ms":model.pressure_fall_per_ms,"poll_interval_ms":20,"release_wait_budget_ms":1000}),
        operation_count: draft.operations.len(),
        completed_operations: completed,
        virtual_elapsed_ms: r.sim.snapshot().at_ms,
        simulated_cut_length_mm: r.cut_length,
        outcome,
        frames: r.frames,
        machine_output_enabled: false,
        hardware_qualified: false,
        limitations: vec![
            "Synthetic plant/supervisor; physical WAZER wiring and hydraulic shutdown are unqualified.",
            motion_scope,
            "Durations rounded up per operation to milliseconds. Rapids use nominal gantry limit. Pierce values are public table estimates.",
            "No automatic resume after faults; pressure release is modeled, not a proven installed sensor.",
        ],
    })
}

pub fn verify(
    original: &[u8],
    integration: &[u8],
    report: &[u8],
    catalog: &Catalog,
) -> Result<serde_json::Value> {
    ensure!(
        report.len() <= 16 * 1024 * 1024,
        "simulation report exceeds 16MiB"
    );
    let value: serde_json::Value = serde_json::from_slice(report)?;
    let fault: FaultSpec = serde_json::from_value(value["fault"].clone())?;
    ensure!(
        crate::json_identity::equivalent(
            &value,
            &serde_json::to_value(replay(original, integration, catalog, fault)?)?
        ),
        "simulation differs from original/owner replay"
    );
    Ok(
        serde_json::json!({"verified":true,"schema":"reversal.waterjet-job-simulation-verification.v1","report_sha256":format!("{:x}",Sha256::digest(report)),"machine_output_enabled":false,"hardware_qualified":false}),
    )
}
