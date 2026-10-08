//! Verified CAM -> shared replay/postprocessor. No controller or machine authority.
use crate::{cam, cutting::Catalog, json_identity, simulation};
use anyhow::{Result, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use swarf_waterjet::wazer;

#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "CamSimulationReplay")]
pub struct Replay {
    pub schema: &'static str,
    pub cam_sha256: String,
    pub replay: simulation::Replay,
    pub machine_output_enabled: bool,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub schema: String,
    pub cam_sha256: String,
    pub project_sha256: String,
    pub source_sha256: String,
    pub tables_canonical_sha256: String,
    pub code: wazer::ResearchCode,
    pub cam_limitations: Vec<String>,
    pub machine_output_enabled: bool,
}

fn bound(artifact: &cam::Artifact, catalog: &Catalog) -> Result<(cam::Artifact, String)> {
    let canonical = cam::canonical(artifact, catalog)?;
    let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&canonical)?));
    Ok((canonical, digest))
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LinuxCncCandidate {
    pub schema: String,
    pub cam_sha256: String,
    pub gcode_sha256: String,
    pub code: swarf_waterjet::linuxcnc::Code,
    pub cam_limitations: Vec<String>,
    pub machine_output_enabled: bool,
}

pub fn linuxcnc_gcode(
    artifact: &cam::Artifact,
    catalog: &Catalog,
    profile: swarf_waterjet::linuxcnc::Profile,
) -> Result<LinuxCncCandidate> {
    let (canonical, cam_sha256) = bound(artifact, catalog)?;
    let plan = cam::verified(&canonical, catalog)?;
    let code = swarf_waterjet::linuxcnc::research_post(&plan.draft, profile)?;
    Ok(LinuxCncCandidate {
        schema: "reversal.openwam-cam-linuxcnc-candidate.v1".into(),
        cam_sha256,
        gcode_sha256: format!("{:x}", Sha256::digest(code.gcode.as_bytes())),
        code,
        cam_limitations: plan.limitations.into_iter().map(str::to_owned).collect(),
        machine_output_enabled: false,
    })
}

pub fn verify_linuxcnc_gcode(
    artifact: &cam::Artifact,
    catalog: &Catalog,
    candidate: &LinuxCncCandidate,
) -> Result<serde_json::Value> {
    let rebuilt = linuxcnc_gcode(artifact, catalog, candidate.code.profile.clone())?;
    ensure!(
        json_identity::equivalent(
            &serde_json::to_value(candidate)?,
            &serde_json::to_value(&rebuilt)?
        ),
        "LinuxCNC candidate differs from independently rebuilt source/intent/table/profile/postprocessor"
    );
    Ok(
        serde_json::json!({"schema":"reversal.openwam-cam-linuxcnc-verification.v1",
        "verified":true,"cam_sha256":rebuilt.cam_sha256,"gcode_sha256":rebuilt.gcode_sha256,
        "span_count":rebuilt.code.span_count,"cut_move_count":rebuilt.code.cut_move_count,
        "machine_output_enabled":false,"controller_qualified":false}),
    )
}

pub fn simulate(
    artifact: &cam::Artifact,
    catalog: &Catalog,
    fault: simulation::FaultSpec,
) -> Result<Replay> {
    let (canonical, cam_sha256) = bound(artifact, catalog)?;
    let plan = cam::verified(&canonical, catalog)?;
    let replay = simulation::replay_draft(
        &plan.draft,
        format!(
            "{:x}",
            Sha256::digest(canonical.project.source_text.as_bytes())
        ),
        cam_sha256.clone(),
        canonical.tables_canonical_sha256,
        fault,
        "Verified compensated CAM moves including tab gaps and leads; straight-line kinematics at nominal table feed, without acceleration, step generation or qualified postprocessing.",
    )?;
    Ok(Replay {
        schema: "reversal.openwam-cam-simulation.v1",
        cam_sha256,
        replay,
        machine_output_enabled: false,
    })
}

pub fn verify_simulation(
    artifact: &cam::Artifact,
    catalog: &Catalog,
    retained: &serde_json::Value,
) -> Result<serde_json::Value> {
    ensure!(
        serde_json::to_vec(retained)?.len() <= 16 * 1024 * 1024,
        "CAM replay exceeds 16 MiB"
    );
    let fault = serde_json::from_value(retained["replay"]["fault"].clone())?;
    let rebuilt = simulate(artifact, catalog, fault)?;
    ensure!(
        json_identity::equivalent(retained, &serde_json::to_value(&rebuilt)?),
        "CAM replay differs from independently rebuilt source/intent/operations"
    );
    Ok(
        serde_json::json!({"schema":"reversal.openwam-cam-simulation-verification.v1","verified":true,"cam_sha256":rebuilt.cam_sha256,"completed_operations":rebuilt.replay.completed_operations,"outcome":rebuilt.replay.outcome,"machine_output_enabled":false,"hardware_qualified":false}),
    )
}

pub fn research_gcode(
    artifact: &cam::Artifact,
    catalog: &Catalog,
    profile: wazer::Profile,
) -> Result<Candidate> {
    let (canonical, cam_sha256) = bound(artifact, catalog)?;
    ensure!(
        profile == wazer::Profile::HistoricalWam16,
        "CAM spans currently require historical_wam16; current Pro service accepts only one closed supplied path"
    );
    let plan = cam::verified(&canonical, catalog)?;
    let code = wazer::research_post(&plan.draft, profile)?;
    Ok(Candidate {
        schema: "reversal.openwam-cam-research-candidate.v1".into(),
        cam_sha256,
        project_sha256: canonical.project_sha256,
        source_sha256: format!(
            "{:x}",
            Sha256::digest(canonical.project.source_text.as_bytes())
        ),
        tables_canonical_sha256: canonical.tables_canonical_sha256,
        code,
        cam_limitations: plan.limitations.into_iter().map(str::to_owned).collect(),
        machine_output_enabled: false,
    })
}

pub fn verify_gcode(
    artifact: &cam::Artifact,
    catalog: &Catalog,
    candidate: &Candidate,
) -> Result<serde_json::Value> {
    let rebuilt = research_gcode(artifact, catalog, candidate.code.profile)?;
    ensure!(
        json_identity::equivalent(
            &serde_json::to_value(candidate)?,
            &serde_json::to_value(&rebuilt)?
        ),
        "CAM candidate differs from independently rebuilt source/intent/table/postprocessor"
    );
    Ok(
        serde_json::json!({"schema":"reversal.openwam-cam-research-verification.v1","verified":true,"cam_sha256":rebuilt.cam_sha256,"gcode_sha256":format!("{:x}",Sha256::digest(rebuilt.code.gcode.as_bytes())),"machine_output_enabled":false,"controller_qualified":false}),
    )
}
