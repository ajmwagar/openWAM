//! Source-bound replay of the actual exported command text, without machine I/O.
use crate::{cam, cam_loop, cutting::Catalog, json_identity, simulation};
use anyhow::{Result, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use swarf_waterjet::wazer;
const MAX_SHUTDOWN_REPLAY_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Serialize, JsonSchema)]
pub struct Replay {
    pub schema: &'static str,
    pub cam_sha256: String,
    pub gcode_sha256: String,
    pub profile: wazer::Profile,
    pub trace: simulation::ProgramTrace,
    pub machine_output_enabled: bool,
    pub controller_qualified: bool,
    pub hardware_qualified: bool,
    pub limitations: Vec<&'static str>,
}

pub fn simulate(
    artifact: &cam::Artifact,
    catalog: &Catalog,
    candidate: &cam_loop::Candidate,
    fault: simulation::FaultSpec,
) -> Result<Replay> {
    cam_loop::verify_gcode(artifact, catalog, candidate)?;
    let rebuilt = cam_loop::research_gcode(artifact, catalog, candidate.code.profile)?;
    let program = wazer::parse(&rebuilt.code.gcode)?;
    let plan = cam::verified(artifact, catalog)?;
    let trace = simulation::replay_program(&program, plan.draft.request.max_feed_mm_min, fault)?;
    Ok(Replay {
        schema: "reversal.openwam-cam-gcode-simulation.v1",
        cam_sha256: rebuilt.cam_sha256,
        gcode_sha256: format!("{:x}", Sha256::digest(rebuilt.code.gcode.as_bytes())),
        profile: rebuilt.code.profile,
        trace,
        machine_output_enabled: false,
        controller_qualified: false,
        hardware_qualified: false,
        limitations: vec![
            "Synthetic controller and pressure model; no WAZER firmware execution or hardware calibration.",
            "Straight-line nominal feeds without acceleration or step generation; rapid feed uses verified CAM maximum feed.",
            "Validated metadata and initialization have zero synthetic duration; all emitted dwells are replayed.",
        ],
    })
}

pub fn verify(
    artifact: &cam::Artifact,
    catalog: &Catalog,
    candidate: &cam_loop::Candidate,
    retained: &serde_json::Value,
) -> Result<serde_json::Value> {
    ensure!(
        serde_json::to_vec(retained)?.len() <= 16 * 1024 * 1024,
        "G-code replay exceeds 16 MiB"
    );
    let fault = serde_json::from_value(retained["trace"]["fault"].clone())?;
    let rebuilt = simulate(artifact, catalog, candidate, fault)?;
    ensure!(
        json_identity::equivalent(retained, &serde_json::to_value(&rebuilt)?),
        "G-code replay differs from independently rebuilt source/export/plant trace"
    );
    Ok(serde_json::json!({
        "schema":"reversal.openwam-cam-gcode-simulation-verification.v1",
        "verified":true,"cam_sha256":rebuilt.cam_sha256,"gcode_sha256":rebuilt.gcode_sha256,
        "completed_commands":rebuilt.trace.completed_commands,"outcome":rebuilt.trace.outcome,
        "machine_output_enabled":false,"controller_qualified":false,"hardware_qualified":false
    }))
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompletionSettings {
    /// Snapshot supplied at M1404 dispatch, not inferred from unmodeled M3/M5 paths.
    pub config_at_dispatch: crate::shutdown::Config,
    pub output_fault: crate::shutdown::Fault,
    /// Relative to dispatch; first handler poll starts the internal timer.
    pub poll_offsets_us: Vec<u64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CompletionReplay {
    pub dispatch_command_index: usize,
    pub dispatch_at_program_us: u64,
    pub last_observation_at_program_us: u64,
    pub relative_replay: crate::shutdown::Replay,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ShutdownReplay {
    pub schema: &'static str,
    pub command_replay: Replay,
    pub completion_settings: CompletionSettings,
    pub completion: Option<CompletionReplay>,
    pub completion_status: &'static str,
    pub machine_output_enabled: bool,
    pub controller_qualified: bool,
    pub hardware_qualified: bool,
    pub limitations: Vec<&'static str>,
}

pub fn simulate_shutdown(
    artifact: &cam::Artifact,
    catalog: &Catalog,
    candidate: &cam_loop::Candidate,
    fault: simulation::FaultSpec,
    settings: &CompletionSettings,
) -> Result<ShutdownReplay> {
    use crate::shutdown::{self, Event, TimedEvent};
    ensure!(
        settings.poll_offsets_us.len() < shutdown::MAX_EVENTS,
        "completion poll budget exceeded"
    );
    let mut events = vec![TimedEvent {
        at_us: 0,
        event: Event::BeginCompletion,
    }];
    events.extend(settings.poll_offsets_us.iter().map(|at_us| TimedEvent {
        at_us: *at_us,
        event: Event::Poll,
    }));
    // Validate caller settings even when the command program will be interrupted.
    let relative_replay = shutdown::simulate(&shutdown::Input {
        config: settings.config_at_dispatch.clone(),
        fault: settings.output_fault,
        events,
    })?;
    let command_replay = simulate(artifact, catalog, candidate, fault)?;
    let dispatch = command_replay.trace.frames.iter().find(|frame| {
        matches!(frame.command, wazer::Command::End)
            && frame.observation.phase == "end"
            && frame.observation.operation < command_replay.trace.completed_commands
    });
    let completion = dispatch.map(|frame| {
        let dispatch_at_program_us = frame.observation.at_ms * 1000;
        CompletionReplay {
            dispatch_command_index: frame.observation.operation,
            dispatch_at_program_us,
            last_observation_at_program_us: dispatch_at_program_us
                + settings.poll_offsets_us.last().copied().unwrap_or(0),
            relative_replay,
        }
    });
    let replay = ShutdownReplay {
        schema: "openwam.cam-gcode-shutdown-replay.v1",
        completion_status: if completion.is_some() {
            "dispatched"
        } else {
            "not_dispatched"
        },
        command_replay,
        completion_settings: settings.clone(),
        completion,
        machine_output_enabled: false,
        controller_qualified: false,
        hardware_qualified: false,
        limitations: vec![
            "Command motion/dwell timing comes from the existing synthetic replay; completion is a separate static shutdown subset.",
            "Six-channel configuration is an explicit snapshot at M1404 dispatch; operating-mode output trajectories are not inferred.",
            "Handler poll offsets are caller supplied; internal completion time is not added to or substituted for emitted dwells.",
            "Interrupted or rejected End commands do not dispatch completion; synthetic faults are not translated into vendor HALT/abort.",
        ],
    };
    ensure!(
        serde_json::to_vec(&replay)?.len() <= MAX_SHUTDOWN_REPLAY_BYTES,
        "combined replay exceeds byte budget"
    );
    Ok(replay)
}

pub fn verify_shutdown(
    artifact: &cam::Artifact,
    catalog: &Catalog,
    candidate: &cam_loop::Candidate,
    fault: simulation::FaultSpec,
    settings: &CompletionSettings,
    retained: &serde_json::Value,
) -> Result<serde_json::Value> {
    ensure!(
        serde_json::to_vec(retained)?.len() <= MAX_SHUTDOWN_REPLAY_BYTES,
        "combined replay exceeds 16 MiB"
    );
    let rebuilt = simulate_shutdown(artifact, catalog, candidate, fault, settings)?;
    ensure!(
        json_identity::equivalent(retained, &serde_json::to_value(&rebuilt)?),
        "combined replay differs from regenerated CAM/export/command/shutdown traces"
    );
    Ok(
        serde_json::json!({"schema":"openwam.cam-gcode-shutdown-verification.v1", "verified":true,
        "cam_sha256":rebuilt.command_replay.cam_sha256,"gcode_sha256":rebuilt.command_replay.gcode_sha256,
        "completion_status":rebuilt.completion_status,"machine_output_enabled":false}),
    )
}
