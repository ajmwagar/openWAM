//! Portable CAM intent and independent owner recomputation; agent API first.
use crate::{bridge, cutting, json_identity, openwam};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use swarf_waterjet::cam::{self, Intent, Plan};
use transmog_core::ir::{Feature, SketchSegment};
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub schema: String,
    pub project: openwam::Project,
    pub intent: Intent,
    pub project_sha256: String,
    pub tables_canonical_sha256: String,
    pub plan: serde_json::Value,
    /// Required stock for a layout project: the union of the placed instance
    /// bounding boxes, not kerf-compensated toolpaths. Omitted without a layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stock: Option<Stock>,
    pub machine_output_enabled: bool,
}

/// Axis-aligned stock envelope of the expanded layout in the Y-down CAD frame.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Stock {
    pub min_xy_mm: [f64; 2],
    pub max_xy_mm: [f64; 2],
    pub width_depth_mm: [f64; 2],
}

/// Union of the expanded placed geometry. Reported alongside the owner's bed
/// check; it is never used to clamp or clip the requested layout.
fn stock(report: &bridge::Integration) -> Result<Option<Stock>> {
    if report.transmog.layout.is_none() {
        return Ok(None);
    }
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for feature in &report.transmog.features {
        let Feature::Sketch { profile, .. } = feature else {
            continue;
        };
        for segment in profile {
            let SketchSegment::Line { start, end } = segment else {
                anyhow::bail!("layout stock requires bounded line profiles")
            };
            for p in [start, end] {
                min = [min[0].min(p.x), min[1].min(p.y)];
                max = [max[0].max(p.x), max[1].max(p.y)];
            }
        }
    }
    ensure!(
        min.iter().chain(&max).all(|v| v.is_finite()),
        "layout stock bounds unavailable"
    );
    Ok(Some(Stock {
        min_xy_mm: min,
        max_xy_mm: max,
        width_depth_mm: [max[0] - min[0], max[1] - min[1]],
    }))
}
pub fn prepare(
    project: &openwam::Project,
    intent: Intent,
    catalog: &cutting::Catalog,
) -> Result<Artifact> {
    let bytes = serde_json::to_vec(project)?;
    let (canonical, report) = openwam::reopen(&bytes, catalog)?;
    let plan = cam::plan(&report.swarf.request, &intent)?;
    let stock = stock(&report)?;
    Ok(Artifact {
        schema: "reversal.openwam-cam.v1".into(),
        project_sha256: format!("{:x}", Sha256::digest(serde_json::to_vec(&canonical)?)),
        tables_canonical_sha256: report.planning.tables_canonical_sha256,
        project: canonical,
        intent,
        plan: serde_json::to_value(plan)?,
        stock,
        machine_output_enabled: false,
    })
}
pub fn verified(artifact: &Artifact, catalog: &cutting::Catalog) -> Result<Plan> {
    let rebuilt = canonical(artifact, catalog)?;
    let (_, report) = openwam::reopen(&serde_json::to_vec(&rebuilt.project)?, catalog)?;
    Ok(cam::plan(&report.swarf.request, &rebuilt.intent)?)
}

/// Return source/owner-rebuilt canonical bytes for downstream artifact binding.
pub(crate) fn canonical(artifact: &Artifact, catalog: &cutting::Catalog) -> Result<Artifact> {
    ensure!(
        artifact.schema == "reversal.openwam-cam.v1" && !artifact.machine_output_enabled,
        "unsupported CAM artifact/authority"
    );
    let rebuilt = prepare(&artifact.project, artifact.intent.clone(), catalog)?;
    ensure!(
        artifact.project_sha256 == rebuilt.project_sha256
            && artifact.tables_canonical_sha256 == rebuilt.tables_canonical_sha256
            && json_identity::equivalent(&artifact.plan, &rebuilt.plan)
            && json_identity::equivalent(
                &serde_json::to_value(&artifact.stock)?,
                &serde_json::to_value(&rebuilt.stock)?
            ),
        "CAM artifact differs from independently rebuilt source/intent/table/owner plan"
    );
    Ok(rebuilt)
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
