//! Portable CAM intent and independent owner recomputation; agent API first.
use crate::{cutting, json_identity, openwam};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use swarf_waterjet::cam::{self, Intent, Plan};
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub schema: String,
    pub project: openwam::Project,
    pub intent: Intent,
    pub project_sha256: String,
    pub tables_canonical_sha256: String,
    pub plan: serde_json::Value,
    pub machine_output_enabled: bool,
}
pub fn prepare(
    project: &openwam::Project,
    intent: Intent,
    catalog: &cutting::Catalog,
) -> Result<Artifact> {
    let bytes = serde_json::to_vec(project)?;
    let (canonical, report) = openwam::reopen(&bytes, catalog)?;
    let plan = cam::plan(&report.swarf.request, &intent)?;
    Ok(Artifact {
        schema: "reversal.openwam-cam.v1".into(),
        project_sha256: format!("{:x}", Sha256::digest(serde_json::to_vec(&canonical)?)),
        tables_canonical_sha256: report.planning.tables_canonical_sha256,
        project: canonical,
        intent,
        plan: serde_json::to_value(plan)?,
        machine_output_enabled: false,
    })
}
pub fn verified(artifact: &Artifact, catalog: &cutting::Catalog) -> Result<Plan> {
    ensure!(
        artifact.schema == "reversal.openwam-cam.v1" && !artifact.machine_output_enabled,
        "unsupported CAM artifact/authority"
    );
    let rebuilt = prepare(&artifact.project, artifact.intent.clone(), catalog)?;
    ensure!(
        artifact.project_sha256 == rebuilt.project_sha256
            && artifact.tables_canonical_sha256 == rebuilt.tables_canonical_sha256
            && json_identity::equivalent(&artifact.plan, &rebuilt.plan),
        "CAM artifact differs from independently rebuilt source/intent/table/owner plan"
    );
    let (_, report) = openwam::reopen(&serde_json::to_vec(&rebuilt.project)?, catalog)?;
    Ok(cam::plan(&report.swarf.request, &rebuilt.intent)?)
}
pub fn verify(artifact: &Artifact, catalog: &cutting::Catalog) -> Result<serde_json::Value> {
    let plan = verified(artifact, catalog)?;
    Ok(
        serde_json::json!({"schema":"reversal.openwam-cam-verification.v1","verified":true,"project_sha256":artifact.project_sha256,"path_count":plan.paths.len(),"operation_count":plan.draft.operations.len(),"machine_output_enabled":false}),
    )
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
pub fn preview(artifact: &Artifact, catalog: &cutting::Catalog) -> Result<String> {
    use std::fmt::Write;
    let plan = verified(artifact, catalog)?;
    let [w, h] = plan.draft.request.cutting_area_width_depth_mm;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}mm\" height=\"{h}mm\" viewBox=\"0 0 {w} {h}\"><title>OpenWAM compensated planning preview</title><desc>Uncut tab gaps and explicit leads; not machine qualified</desc><rect width=\"{w}\" height=\"{h}\" fill=\"white\"/>"
    );
    // Source boundaries provide context while actual derived moves are drawn below.
    let (_, source) = openwam::reopen(&serde_json::to_vec(&artifact.project)?, catalog)?;
    for c in &source.swarf.request.contours {
        let mut pts = String::new();
        for segment in &c.profile {
            if let transmog_core::ir::SketchSegment::Line { start, .. } = segment {
                write!(pts, "{},{} ", start.x, -start.y)?;
            }
        }
        if let Some(transmog_core::ir::SketchSegment::Line { end, .. }) = c.profile.last() {
            write!(pts, "{},{}", end.x, -end.y)?;
        }
        write!(
            svg,
            "<polyline points=\"{pts}\" fill=\"none\" stroke=\"#999\" stroke-width=\"0.1\"><title>{}</title></polyline>",
            escape(&c.source_id)
        )?;
    }
    let mut position: Option<[f64; 2]> = None;
    for op in &plan.draft.operations {
        match op {
            swarf_waterjet::Operation::Rapid { to_mm } => {
                if let Some([x, y]) = position {
                    write!(
                        svg,
                        "<path d=\"M {x} {} L {} {}\" fill=\"none\" stroke=\"#888\" stroke-dasharray=\"1 1\" stroke-width=\"0.15\"/>",
                        -y, to_mm[0], -to_mm[1]
                    )?;
                }
                position = Some(*to_mm);
            }
            swarf_waterjet::Operation::Cut { to_mm, .. } => {
                let [x, y] = position.context("cut missing start")?;
                write!(
                    svg,
                    "<path d=\"M {x} {} L {} {}\" fill=\"none\" stroke=\"#008faf\" stroke-width=\"0.25\"/>",
                    -y, to_mm[0], -to_mm[1]
                )?;
                position = Some(*to_mm);
            }
            swarf_waterjet::Operation::Pierce { .. } => {
                let [x, y] = position.context("pierce missing position")?;
                write!(
                    svg,
                    "<circle cx=\"{x}\" cy=\"{}\" r=\"0.45\" fill=\"#db8500\"/>",
                    -y
                )?;
            }
            _ => {}
        }
    }
    svg.push_str("</svg>");
    Ok(svg)
}
