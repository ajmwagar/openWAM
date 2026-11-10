//! Verified openWAM CAM artifact -> WAM 2.4 `.wzr` project.
//!
//! The artifact and the `verify_cam` response are plain JSON produced by the
//! `openwam` CLI in a separate process; nothing here links openWAM's AGPL crate.
//! A `.wzr` is a WAM *project*: WAM recomputes kerf offsets, leads, pierces and
//! G-code from it. Nothing in this module produces machine commands.

use crate::bbox::wam_file_bb;
use crate::json::js_number;
use crate::model::*;
use serde::Serialize;
use serde_json::Value;

/// Kerf offset WAM wrote in every sample (inches), independent of material.
pub const WAM_OFFSET_DISTANCE_IN: f64 = 0.019;
/// Eco/Speed feed ratio of the only WAM-native Pro material in the samples
/// (Polyurethane 0.5 in: 2.756 / 29.1). Used only to keep eco mode slower.
pub const ECO_FEED_RATIO: f64 = 2.756 / 29.1;

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("refused: verify_cam did not pass for this artifact ({0})")]
    NotVerified(String),
    #[error("artifact field missing or malformed: {0}")]
    Artifact(String),
    #[error(
        "refused: contours need both kept-part and kept-drop offsets; WAM applies one offset per group (splitGroups unverified)"
    )]
    MixedSides,
    #[error("refused: no cut contours in the plan")]
    NothingToCut,
    #[error(transparent)]
    BBox(#[from] crate::bbox::BBoxError),
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportReport {
    pub verified_project_sha256: String,
    pub offset_type: OffsetType,
    pub cut_contours: Vec<String>,
    pub dropped_no_cut_contours: Vec<String>,
    /// openWAM tabs that are not carried into the .wzr (WAM tab type `none`).
    pub dropped_tabs: usize,
    pub file_bb_in: [f64; 4],
    pub position_in: [f64; 2],
    pub feed: f64,
    pub feed_unit: String,
    pub pierce_s: f64,
    pub lead_mm: f64,
    pub unverified: Vec<&'static str>,
    pub machine_output_enabled: bool,
    pub hardware_qualified: bool,
}

fn get<'a>(v: &'a Value, path: &[&str]) -> Result<&'a Value, ExportError> {
    let mut cur = v;
    for k in path {
        cur = cur
            .get(*k)
            .ok_or_else(|| ExportError::Artifact(path.join(".")))?;
    }
    Ok(cur)
}

fn num(v: &Value, path: &[&str]) -> Result<f64, ExportError> {
    get(v, path)?
        .as_f64()
        .ok_or_else(|| ExportError::Artifact(path.join(".")))
}

fn text<'a>(v: &'a Value, path: &[&str]) -> Result<&'a str, ExportError> {
    get(v, path)?
        .as_str()
        .ok_or_else(|| ExportError::Artifact(path.join(".")))
}

/// The gate: the `verify_cam` response must be an ok verification of exactly this artifact.
pub fn check_verification(
    artifact: &Value,
    verify_response: &Value,
) -> Result<String, ExportError> {
    let nv = |m: &str| ExportError::NotVerified(m.into());
    if verify_response["status"] != "ok" {
        return Err(nv("verify_cam status is not ok"));
    }
    let v = &verify_response["result"]["verification"];
    if verify_response["result"]["kind"] != "verification" || v["verified"] != Value::Bool(true) {
        return Err(nv("verification.verified is not true"));
    }
    if v["schema"] != "reversal.openwam-cam-verification.v1" {
        return Err(nv("not a CAM verification receipt"));
    }
    let sha = v["project_sha256"]
        .as_str()
        .ok_or_else(|| nv("receipt has no project_sha256"))?;
    if artifact["project_sha256"].as_str() != Some(sha) {
        return Err(nv("receipt project_sha256 does not match the artifact"));
    }
    if artifact["machine_output_enabled"] != Value::Bool(false)
        || v["machine_output_enabled"] != Value::Bool(false)
    {
        return Err(nv("artifact claims machine output; refusing"));
    }
    Ok(sha.to_owned())
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn n(v: f64) -> Result<String, ExportError> {
    js_number(v).ok_or_else(|| ExportError::Artifact("non-finite coordinate".into()))
}

fn pt(p: &Value) -> Result<String, ExportError> {
    let x = p
        .get(0)
        .and_then(Value::as_f64)
        .ok_or_else(|| ExportError::Artifact("point".into()))?;
    let y = p
        .get(1)
        .and_then(Value::as_f64)
        .ok_or_else(|| ExportError::Artifact("point".into()))?;
    Ok(format!("{} {}", n(x)?, n(y)?))
}

fn path_data(segments: &Value) -> Result<String, ExportError> {
    let mut d = Vec::new();
    for s in segments
        .as_array()
        .ok_or_else(|| ExportError::Artifact("segments".into()))?
    {
        d.push(match s["kind"].as_str() {
            Some("move") => format!("M{}", pt(&s["to"])?),
            Some("line") => format!("L{}", pt(&s["to"])?),
            Some("quadratic") => format!("Q{} {}", pt(&s["control"])?, pt(&s["to"])?),
            Some("cubic") => format!(
                "C{} {} {}",
                pt(&s["controls"][0])?,
                pt(&s["controls"][1])?,
                pt(&s["to"])?
            ),
            Some("close") => "Z".into(),
            other => return Err(ExportError::Artifact(format!("segment kind {other:?}"))),
        });
    }
    Ok(d.join(""))
}

fn bbox(r: kurbo::Rect, originals: Option<[f64; 4]>) -> BBox {
    let [omnx, omny, omxx, omxy] = originals.map_or([None; 4], |o| o.map(Some));
    BBox {
        max_x: Some(r.x1),
        max_y: Some(r.y1),
        min_x: Some(r.x0),
        min_y: Some(r.y0),
        width: Some(r.x1 - r.x0),
        height: Some(r.y1 - r.y0),
        original_min_x: omnx,
        original_min_y: omny,
        original_max_x: omxx,
        original_max_y: omxy,
    }
}

/// Build a `.wzr` project from a verified `plan_cam` artifact.
pub fn export(
    artifact: &Value,
    verify_response: &Value,
    filename: &str,
) -> Result<(Project, ExportReport), ExportError> {
    let sha = check_verification(artifact, verify_response)?;
    let planning = get(artifact, &["project", "integration", "planning"])?;
    let sketch = get(planning, &["sketch"])?;
    if text(sketch, &["units"])? != "mm"
        || text(sketch, &["coordinate_system"])? != "svg_top_left_y_down"
    {
        return Err(ExportError::Artifact(
            "sketch must be mm, svg_top_left_y_down".into(),
        ));
    }
    let viewport = |i: usize| {
        sketch["viewport_mm"][i]
            .as_f64()
            .ok_or_else(|| ExportError::Artifact("sketch.viewport_mm".into()))
    };
    let (vw, vh) = (viewport(0)?, viewport(1)?);

    // Sides per source contour from the verified plan.
    let paths = get(artifact, &["plan", "paths"])?
        .as_array()
        .ok_or_else(|| ExportError::Artifact("plan.paths".into()))?;
    let mut keep_part = None::<bool>;
    let mut cut_ids = Vec::new();
    for p in paths {
        let side = text(p, &["side"])?;
        let depth = get(p, &["containment_depth"])?
            .as_u64()
            .ok_or_else(|| ExportError::Artifact("containment_depth".into()))?;
        let this = match side {
            "outside" => depth % 2 == 0,
            "inside" => depth % 2 == 1,
            _ => return Err(ExportError::Artifact(format!("plan side {side:?}"))),
        };
        if *keep_part.get_or_insert(this) != this {
            return Err(ExportError::MixedSides);
        }
        cut_ids.push(text(p, &["source_id"])?.to_owned());
    }
    let keep_part = keep_part.ok_or(ExportError::NothingToCut)?;
    let offset_type = if keep_part {
        OffsetType::Outside
    } else {
        OffsetType::Inside
    };

    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}mm" height="{h}mm" viewBox="0 0 {w} {h}">"#,
        w = n(vw)?,
        h = n(vh)?
    );
    let mut dropped = Vec::new();
    for c in sketch["contours"]
        .as_array()
        .ok_or_else(|| ExportError::Artifact("sketch.contours".into()))?
    {
        let id = text(c, &["source_id"])?;
        if cut_ids.iter().any(|x| x == id) {
            svg += &format!(
                "\n<path id=\"{}\" d=\"{}\"/>",
                xml_escape(id),
                path_data(&c["segments"])?
            );
        } else {
            dropped.push(id.to_owned());
        }
    }
    svg += "\n</svg>\n";
    if dropped.len() + cut_ids.len() != sketch["contours"].as_array().map_or(0, Vec::len) {
        return Err(ExportError::Artifact(
            "plan paths do not match sketch contours".into(),
        ));
    }

    let fb = wam_file_bb(&svg)?;
    let file_bb = bbox(fb, Some([0.0; 4]));
    let (cx, cy) = ((fb.x0 + fb.x1) / 2.0, (fb.y0 + fb.y1) / 2.0);
    let (hw, hh) = ((fb.x1 - fb.x0) / 2.0, (fb.y1 - fb.y0) / 2.0);
    let rst = kurbo::Rect::new(-hw, -hh, hw, hh);
    let rst_bb = bbox(rst, Some([rst.x0, rst.y0, rst.x1, rst.y1]));
    let place = get(
        artifact,
        &["project", "integration", "transmog", "placement_xy_down_mm"],
    )?;
    let (px, py) = (
        place[0]
            .as_f64()
            .ok_or_else(|| ExportError::Artifact("placement".into()))?,
        place[1]
            .as_f64()
            .ok_or_else(|| ExportError::Artifact("placement".into()))?,
    );
    let position = Point {
        x: px / 25.4 + cx,
        y: py / 25.4 + cy,
    };

    // Material from the catalog row openWAM verified against.
    let row = get(planning, &["selected_row"])?;
    let metric = text(row, &["units"])? == "metric";
    let (lu, unit, rate_unit) = if metric {
        (LengthUnit::Mm, "mm", "mm / min")
    } else {
        (LengthUnit::In, "in", "in / min")
    };
    let feed = num(row, &["cut", "feed"])?;
    let pierce = num(row, &["cut", "pierce_seconds"])?;
    let machine = match text(row, &["model"])? {
        "pro" => MachineType::Pro,
        "desktop" => MachineType::Desktop,
        m => return Err(ExportError::Artifact(format!("model {m:?}"))),
    };
    let abrasive_g_min = num(planning, &["machine_limits", "abrasive_kg_min"])? * 1000.0;
    let rates = |name: &str, f: f64| Rates {
        name: name.into(),
        abrasive_rate: MathUnit::new(abrasive_g_min, "g / min"),
        rough_rate: MathUnit::new(f, rate_unit),
        medium_rate: MathUnit::new(f, rate_unit),
        fine_rate: MathUnit::new(f, rate_unit),
    };
    let intent = get(artifact, &["intent", "contours"])?
        .as_array()
        .cloned()
        .unwrap_or_default();
    let cut_intent = || intent.iter().filter(|c| c["side"] != "no_cut");
    let lead_mm = cut_intent()
        .flat_map(|c| [c["lead_in_mm"].as_f64(), c["lead_out_mm"].as_f64()])
        .flatten()
        .fold(0.0, f64::max);
    let dropped_tabs = cut_intent()
        .map(|c| c["tabs"].as_array().map_or(0, Vec::len))
        .sum();
    let lead_value = if metric { lead_mm } else { lead_mm / 25.4 };
    let material_name = text(row, &["material"])?;
    let label = text(row, &["thickness_label"]).unwrap_or("");
    let material = Material {
        user_def: true,
        u_id: None,
        name: format!("{material_name} {label} (openWAM)"),
        units: lu,
        category: "openWAM".into(),
        thickness: MathUnit::new(num(row, &["thickness"])?, unit),
        speed_mode: rates("Speed Mode", feed),
        eco_mode: rates("Eco Mode", feed * ECO_FEED_RATIO),
        pierce_time: MathUnit::new(pierce, "s"),
        default_tab_width: MathUnit::new(0.0, unit),
        default_lead_dist: MathUnit::new(lead_value, unit),
        machine_type: machine,
        max_tab: (machine == MachineType::Pro).then_some(3),
    };

    let project = Project {
        wam_version: Some("2.4.0".into()),
        file_list: FileList {
            files: vec![FileEntry {
                filename: filename.into(),
                file_id: 0,
                units: LengthUnit::Mm,
                svg_string: svg,
                split_groups: false,
                group_instances: vec![vec![0]],
                instances: 1,
                file_bb: file_bb.clone(),
            }],
            file_id_counter: 1,
        },
        selected_material: SelectedMaterial {
            material,
            quality: Quality::Medium,
            abrasive_mode: AbrasiveMode::Speed,
        },
        offsets: vec![Offset {
            group_id: 0,
            kind: offset_type,
            distance: WAM_OFFSET_DISTANCE_IN,
            file_id: 0,
        }],
        tabs: vec![Tab {
            group_id: 0,
            file_id: Some(0),
            kind: TabType::None,
            tab_count: 1,
            tab_size: 0.0,
            enable_leads: lead_mm > 0.0,
            cut_type: Some(CutType::Standard),
            scored_tab_speed_multiplier: Some(3.0),
            tab_locations: vec![vec![]],
        }],
        cut_quality: Quality::Medium,
        cut_order: Some(CutOrder {
            mode: CutOrderMode::Automatic,
            groups: vec![CutOrderGroup {
                path_group_id: 0,
                segments: vec![()],
            }],
        }),
        display_unit: Some(LengthUnit::In),
        placements: vec![Placement {
            file_id: 0,
            base_bb: file_bb,
            rst_bb,
            instances: vec![Instance {
                scale: 1.0,
                rotation: 0.0,
                position,
            }],
        }],
    };
    let mut unverified = vec![
        "material uId null with userDef true (WAM-native user materials carry a server uId)",
        "bed position: assumes WAM bed origin top-left, y down, inches",
        "RSTBB centred on the exact geometry bbox centre (WAM's centre differs by up to 3.4e-5 in)",
        "eco-mode feeds = speed feed x 0.0947 (ratio from the one WAM Pro sample)",
    ];
    if offset_type == OffsetType::Outside {
        unverified.push("offset type Outside has not appeared in a WAM-saved sample");
    }
    if dropped_tabs > 0 {
        unverified.push("openWAM tabs are not written; WAM cuts this part without tabs");
    }
    let report = ExportReport {
        verified_project_sha256: sha,
        offset_type,
        cut_contours: cut_ids,
        dropped_no_cut_contours: dropped,
        dropped_tabs,
        file_bb_in: [fb.x0, fb.y0, fb.x1, fb.y1],
        position_in: [position.x, position.y],
        feed,
        feed_unit: rate_unit.into(),
        pierce_s: pierce,
        lead_mm,
        unverified,
        machine_output_enabled: false,
        hardware_qualified: false,
    };
    Ok((project, report))
}

#[cfg(test)]
mod tests {
    use super::{ExportError, check_verification};
    use serde_json::json;

    fn receipt(verified: bool, sha: &str) -> serde_json::Value {
        json!({"status": "ok", "machine_output_enabled": false, "result": {"kind": "verification", "verification": {
            "schema": "reversal.openwam-cam-verification.v1", "verified": verified, "project_sha256": sha,
            "machine_output_enabled": false}}})
    }

    #[test]
    fn export_gate_requires_a_matching_passed_verification() {
        let artifact = json!({"project_sha256": "abc", "machine_output_enabled": false});
        assert!(check_verification(&artifact, &receipt(true, "abc")).is_ok());
        for bad in [
            receipt(false, "abc"),
            receipt(true, "other"),
            json!({"status": "error", "error": {"code": "cam_invalid"}}),
        ] {
            assert!(matches!(
                check_verification(&artifact, &bad),
                Err(ExportError::NotVerified(_))
            ));
        }
        let machine = json!({"project_sha256": "abc", "machine_output_enabled": true});
        assert!(matches!(
            check_verification(&machine, &receipt(true, "abc")),
            Err(ExportError::NotVerified(_))
        ));
    }
}
