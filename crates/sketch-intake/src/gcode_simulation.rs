//! Source-bound replay of the actual exported command text, without machine I/O.
use crate::{cam, cam_loop, cutting::Catalog, json_identity, simulation};
use anyhow::{Result, ensure};
use schemars::JsonSchema;
use serde::Serialize;
use sha2::{Digest, Sha256};
use swarf_waterjet::wazer;

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
