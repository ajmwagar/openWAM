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
    #[error(
        "refused: {file} copy {copy} has rotation {degrees} deg; no WAM sample shows how WAM stores a rotated group"
    )]
    Rotation {
        file: String,
        copy: usize,
        degrees: f64,
    },
    #[error(
        "refused: {file} cuts different contours in different copies; a WAM file holds one drawing for all of its groups"
    )]
    InconsistentCuts { file: String },
    #[error("refused: user material {0}")]
    Material(String),
    #[error(transparent)]
    BBox(#[from] crate::bbox::BBoxError),
}

/// What one `.wzr` file entry carries.
#[derive(Debug, Clone, Serialize)]
pub struct FileReport {
    pub file_id: u64,
    pub filename: String,
    pub cut_contours: Vec<String>,
    pub dropped_no_cut_contours: Vec<String>,
    pub file_bb_in: [f64; 4],
    /// Group id, offset type, scale and bed position (inches) per copy.
    pub groups: Vec<GroupReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupReport {
    pub group_id: u64,
    pub offset_type: OffsetType,
    pub scale: f64,
    pub position_in: [f64; 2],
    pub leads: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportReport {
    pub verified_project_sha256: String,
    pub files: Vec<FileReport>,
    /// Source files with nothing to cut, left out of the .wzr.
    pub omitted_files: Vec<String>,
    pub material_user_defined: bool,
    /// openWAM tabs that are not carried into the .wzr (WAM tab type `none`).
    pub dropped_tabs: usize,
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

/// One drawing in the `.wzr`: a source file and the copies placed on the bed.
struct FileSpec {
    name: String,
    /// `"<source id>:"` for a bundle source, empty for a single source.
    prefix: String,
    viewport: [f64; 2],
    layout_applied: bool,
    /// Sketch contour ids without the bundle prefix or copy suffix, in sketch order.
    base_ids: Vec<String>,
    /// `(scale, rotation_deg)` per copy.
    copies: Vec<(f64, f64)>,
}

impl FileSpec {
    fn placed_id(&self, base: &str, copy: usize) -> String {
        let suffix = if self.layout_applied {
            format!("#{copy}")
        } else {
            String::new()
        };
        format!("{}{base}{suffix}", self.prefix)
    }
}

fn file_specs(
    integration: &Value,
    sketch: &Value,
    filename: &str,
) -> Result<Vec<FileSpec>, ExportError> {
    let art = |m: &str| ExportError::Artifact(m.into());
    let contours = sketch["contours"]
        .as_array()
        .ok_or_else(|| art("sketch.contours"))?;
    let ids: Vec<&str> = contours
        .iter()
        .map(|c| {
            c["source_id"]
                .as_str()
                .ok_or_else(|| art("contour source_id"))
        })
        .collect::<Result<_, _>>()?;
    let transmog = &integration["transmog"];
    if let Some(sources) = transmog["sources"].as_array() {
        let mut specs = Vec::new();
        for s in sources {
            let id = s["id"].as_str().ok_or_else(|| art("source id"))?;
            let prefix = format!("{id}:");
            let copies = s["instances"]
                .as_array()
                .ok_or_else(|| art("source instances"))?
                .iter()
                .map(|i| {
                    Ok((
                        i["scale"].as_f64().ok_or_else(|| art("instance scale"))?,
                        i["rotation_deg"]
                            .as_f64()
                            .ok_or_else(|| art("instance rotation"))?,
                    ))
                })
                .collect::<Result<Vec<_>, ExportError>>()?;
            let viewport = |i: usize| {
                s["viewport_mm"][i]
                    .as_f64()
                    .ok_or_else(|| art("source viewport_mm"))
            };
            specs.push(FileSpec {
                name: s["name"].as_str().ok_or_else(|| art("source name"))?.into(),
                viewport: [viewport(0)?, viewport(1)?],
                layout_applied: s["layout_applied"].as_bool().unwrap_or(false),
                base_ids: ids
                    .iter()
                    .filter_map(|c| c.strip_prefix(&prefix))
                    .map(str::to_owned)
                    .collect(),
                prefix,
                copies,
            });
        }
        return Ok(specs);
    }
    let vp = |i: usize| {
        sketch["viewport_mm"][i]
            .as_f64()
            .ok_or_else(|| art("sketch.viewport_mm"))
    };
    let layout = &transmog["layout"];
    let copies = match layout["instances"].as_array() {
        Some(list) => list
            .iter()
            .map(|i| {
                Ok((
                    i["scale"].as_f64().ok_or_else(|| art("layout scale"))?,
                    i["rotation_deg"]
                        .as_f64()
                        .ok_or_else(|| art("layout rotation"))?,
                ))
            })
            .collect::<Result<Vec<_>, ExportError>>()?,
        None => vec![(1.0, 0.0)],
    };
    Ok(vec![FileSpec {
        name: filename.into(),
        prefix: String::new(),
        viewport: [vp(0)?, vp(1)?],
        layout_applied: layout["instances"].is_array(),
        base_ids: ids.iter().map(|s| (*s).to_owned()).collect(),
        copies,
    }])
}

/// Bed-frame (Y down) bounding box `[min_x, min_y, max_x, max_y]` in mm of every
/// placed contour, from openWAM's own verified placed geometry.
fn placed_boxes(
    transmog: &Value,
) -> Result<std::collections::BTreeMap<String, [f64; 4]>, ExportError> {
    let art = |m: &str| ExportError::Artifact(m.into());
    let features = transmog["features"]
        .as_array()
        .ok_or_else(|| art("transmog.features"))?;
    let identity = transmog["contour_identity"]
        .as_array()
        .ok_or_else(|| art("transmog.contour_identity"))?;
    if features.len() != identity.len() {
        return Err(art("features and contour_identity differ in length"));
    }
    let mut out = std::collections::BTreeMap::new();
    for (feature, id) in features.iter().zip(identity) {
        let id = id["source_id"]
            .as_str()
            .ok_or_else(|| art("identity source_id"))?;
        let mut b = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for seg in feature["Sketch"]["profile"]
            .as_array()
            .ok_or_else(|| art("feature profile"))?
        {
            for end in ["start", "end"] {
                let p = &seg["Line"][end];
                let (x, y) = (
                    p[0].as_f64().ok_or_else(|| art("feature point"))?,
                    p[1].as_f64().ok_or_else(|| art("feature point"))?,
                );
                b = [b[0].min(x), b[1].min(-y), b[2].max(x), b[3].max(-y)];
            }
        }
        out.insert(id.to_owned(), b);
    }
    Ok(out)
}

struct Recipe {
    material: Material,
    quality: Quality,
    feed: f64,
    pierce: f64,
    rate_unit: &'static str,
    user_defined: bool,
}

fn recipe(planning: &Value, intent_lead_mm: f64) -> Result<Recipe, ExportError> {
    let row = get(planning, &["selected_row"])?;
    let metric = text(row, &["units"])? == "metric";
    let (lu, unit, rate_unit) = if metric {
        (LengthUnit::Mm, "mm", "mm / min")
    } else {
        (LengthUnit::In, "in", "in / min")
    };
    let machine = match text(row, &["model"])? {
        "pro" => MachineType::Pro,
        "desktop" => MachineType::Desktop,
        m => return Err(ExportError::Artifact(format!("model {m:?}"))),
    };
    let machine_abrasive = num(planning, &["machine_limits", "abrasive_kg_min"])? * 1000.0;
    let feed = num(row, &["cut", "feed"])?;
    let pierce = num(row, &["cut", "pierce_seconds"])?;
    let user = &planning["user_material"];
    let mu = |v: f64, u: &str| MathUnit::new(v, u);
    if user.is_object() {
        if user["user_defined"] != Value::Bool(true) || row["user_defined"] != Value::Bool(true) {
            return Err(ExportError::Material("is not marked user_defined".into()));
        }
        let wam = &user["wam"];
        let arr = |v: &Value, what: &str| -> Result<[f64; 3], ExportError> {
            let a = v.as_array().filter(|a| a.len() == 3);
            let g = |i: usize| a.and_then(|a| a[i].as_f64());
            match (g(0), g(1), g(2)) {
                (Some(x), Some(y), Some(z)) => Ok([x, y, z]),
                _ => Err(ExportError::Material(format!(
                    "{what} is not three numbers"
                ))),
            }
        };
        let (speed, eco) = if wam.is_object() {
            (
                arr(&wam["speed_rates"], "speed_rates")?,
                arr(&wam["eco_rates"], "eco_rates")?,
            )
        } else {
            ([feed; 3], [feed * ECO_FEED_RATIO; 3])
        };
        let abrasive = if wam.is_object() {
            num(wam, &["abrasive_g_min"])?
        } else {
            machine_abrasive
        };
        let quality = match text(user, &["cut_rate"])? {
            "coarse" => Quality::Rough,
            "medium" => Quality::Medium,
            "fine" => Quality::Fine,
            other => return Err(ExportError::Material(format!("cut_rate {other:?}"))),
        };
        let rates = |name: &str, r: [f64; 3]| Rates {
            name: name.into(),
            abrasive_rate: mu(abrasive, "g / min"),
            rough_rate: mu(r[0], rate_unit),
            medium_rate: mu(r[1], rate_unit),
            fine_rate: mu(r[2], rate_unit),
        };
        let max_tab = if wam.is_object() {
            wam["max_tab"].as_u64()
        } else {
            (machine == MachineType::Pro).then_some(3)
        };
        let material = Material {
            user_def: true,
            u_id: wam["uid"].as_u64(),
            name: text(user, &["name"])?.into(),
            units: lu,
            category: text(user, &["category"])?.into(),
            thickness: mu(num(row, &["thickness"])?, unit),
            speed_mode: rates("Speed Mode", speed),
            eco_mode: rates("Eco Mode", eco),
            pierce_time: mu(pierce, "s"),
            default_tab_width: mu(num(user, &["tab_size"])?, unit),
            default_lead_dist: mu(num(user, &["lead_length"])?, unit),
            machine_type: machine,
            max_tab,
        };
        return Ok(Recipe {
            material,
            quality,
            feed,
            pierce,
            rate_unit,
            user_defined: true,
        });
    }
    let rates = |name: &str, f: f64| Rates {
        name: name.into(),
        abrasive_rate: mu(machine_abrasive, "g / min"),
        rough_rate: mu(f, rate_unit),
        medium_rate: mu(f, rate_unit),
        fine_rate: mu(f, rate_unit),
    };
    let lead_value = if metric {
        intent_lead_mm
    } else {
        intent_lead_mm / 25.4
    };
    let label = text(row, &["thickness_label"]).unwrap_or("");
    let material = Material {
        user_def: true,
        u_id: None,
        name: format!("{} {label} (openWAM)", text(row, &["material"])?),
        units: lu,
        category: "openWAM".into(),
        thickness: mu(num(row, &["thickness"])?, unit),
        speed_mode: rates("Speed Mode", feed),
        eco_mode: rates("Eco Mode", feed * ECO_FEED_RATIO),
        pierce_time: mu(pierce, "s"),
        default_tab_width: mu(0.0, unit),
        default_lead_dist: mu(lead_value, unit),
        machine_type: machine,
        max_tab: (machine == MachineType::Pro).then_some(3),
    };
    Ok(Recipe {
        material,
        quality: Quality::Medium,
        feed,
        pierce,
        rate_unit,
        user_defined: false,
    })
}

/// Build a `.wzr` project from a verified `plan_cam` artifact. A bundle exports
/// one file entry per source; a single source with a layout exports one entry
/// with one group per copy.
pub fn export(
    artifact: &Value,
    verify_response: &Value,
    filename: &str,
) -> Result<(Project, ExportReport), ExportError> {
    let sha = check_verification(artifact, verify_response)?;
    let integration = get(artifact, &["project", "integration"])?;
    let planning = get(integration, &["planning"])?;
    let sketch = get(planning, &["sketch"])?;
    if text(sketch, &["units"])? != "mm"
        || text(sketch, &["coordinate_system"])? != "svg_top_left_y_down"
    {
        return Err(ExportError::Artifact(
            "sketch must be mm, svg_top_left_y_down".into(),
        ));
    }
    let specs = file_specs(integration, sketch, filename)?;
    let boxes = placed_boxes(get(integration, &["transmog"])?)?;

    // Sides per placed contour from the verified plan.
    let paths = get(artifact, &["plan", "paths"])?
        .as_array()
        .ok_or_else(|| ExportError::Artifact("plan.paths".into()))?;
    let mut cut: std::collections::BTreeMap<&str, (&str, u64)> = std::collections::BTreeMap::new();
    for p in paths {
        let depth = get(p, &["containment_depth"])?
            .as_u64()
            .ok_or_else(|| ExportError::Artifact("containment_depth".into()))?;
        cut.insert(text(p, &["source_id"])?, (text(p, &["side"])?, depth));
    }
    let intent = get(artifact, &["intent", "contours"])?
        .as_array()
        .cloned()
        .unwrap_or_default();
    let lead_of = |id: &str| -> f64 {
        intent
            .iter()
            .filter(|c| c["source_id"] == id && c["side"] != "no_cut")
            .flat_map(|c| [c["lead_in_mm"].as_f64(), c["lead_out_mm"].as_f64()])
            .flatten()
            .fold(0.0, f64::max)
    };
    let tabs_of = |id: &str| -> usize {
        intent
            .iter()
            .filter(|c| c["source_id"] == id && c["side"] != "no_cut")
            .map(|c| c["tabs"].as_array().map_or(0, Vec::len))
            .sum()
    };
    let lead_mm = intent
        .iter()
        .filter(|c| c["side"] != "no_cut")
        .flat_map(|c| [c["lead_in_mm"].as_f64(), c["lead_out_mm"].as_f64()])
        .flatten()
        .fold(0.0, f64::max);
    let recipe = recipe(planning, lead_mm)?;

    struct Group {
        id: u64,
        file_id: u64,
        offset: OffsetType,
        leads: bool,
    }
    let mut files = Vec::new();
    let mut placements = Vec::new();
    let mut groups: Vec<Group> = Vec::new();
    let mut reports = Vec::new();
    let mut omitted = Vec::new();
    let mut dropped_tabs = 0;
    let mut next_group = 0u64;
    for spec in &specs {
        // Which contours are cut: identical in every copy, or the file cannot be one drawing.
        let mut cut_set: Option<Vec<&String>> = None;
        for copy in 0..spec.copies.len() {
            let here: Vec<&String> = spec
                .base_ids
                .iter()
                .filter(|b| cut.contains_key(spec.placed_id(b, copy).as_str()))
                .collect();
            if *cut_set.get_or_insert_with(|| here.clone()) != here {
                return Err(ExportError::InconsistentCuts {
                    file: spec.name.clone(),
                });
            }
        }
        let cut_base = cut_set.unwrap_or_default();
        if cut_base.is_empty() {
            omitted.push(spec.name.clone());
            continue;
        }
        let file_id = files.len() as u64;
        let (vw, vh) = (spec.viewport[0], spec.viewport[1]);
        let mut svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}mm" height="{h}mm" viewBox="0 0 {w} {h}">"#,
            w = n(vw)?,
            h = n(vh)?
        );
        let mut dropped = Vec::new();
        let mut cut_names = Vec::new();
        for c in sketch["contours"]
            .as_array()
            .ok_or_else(|| ExportError::Artifact("sketch.contours".into()))?
        {
            let full = text(c, &["source_id"])?;
            let Some(base) = full.strip_prefix(spec.prefix.as_str()) else {
                continue;
            };
            if cut_base.iter().any(|b| b.as_str() == base) {
                svg += &format!(
                    "\n<path id=\"{}\" d=\"{}\"/>",
                    xml_escape(base),
                    path_data(&c["segments"])?
                );
                cut_names.push(base.to_owned());
            } else {
                dropped.push(base.to_owned());
            }
        }
        svg += "\n</svg>\n";
        let fb = wam_file_bb(&svg)?;
        let file_bb = bbox(fb, Some([0.0; 4]));
        let (hw, hh) = ((fb.x1 - fb.x0) / 2.0, (fb.y1 - fb.y0) / 2.0);

        let mut instances = Vec::new();
        let mut group_ids = Vec::new();
        let mut group_reports = Vec::new();
        for (copy, (scale, rotation)) in spec.copies.iter().enumerate() {
            if *rotation != 0.0 {
                return Err(ExportError::Rotation {
                    file: spec.name.clone(),
                    copy,
                    degrees: *rotation,
                });
            }
            // One offset per group: every cut contour of the copy must agree.
            let mut kind: Option<OffsetType> = None;
            let mut area = [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ];
            let mut leads = 0.0f64;
            for base in &cut_base {
                let id = spec.placed_id(base, copy);
                let (side, depth) = cut[id.as_str()];
                let this = match side {
                    "outside" => {
                        if depth % 2 == 0 {
                            OffsetType::Outside
                        } else {
                            OffsetType::Inside
                        }
                    }
                    "inside" => {
                        if depth % 2 == 1 {
                            OffsetType::Outside
                        } else {
                            OffsetType::Inside
                        }
                    }
                    "centerline" => OffsetType::Centerline,
                    _ => return Err(ExportError::Artifact(format!("plan side {side:?}"))),
                };
                if *kind.get_or_insert(this) != this {
                    return Err(ExportError::MixedSides);
                }
                let b = boxes
                    .get(&id)
                    .ok_or_else(|| ExportError::Artifact(format!("placed geometry for {id}")))?;
                area = [
                    area[0].min(b[0]),
                    area[1].min(b[1]),
                    area[2].max(b[2]),
                    area[3].max(b[3]),
                ];
                leads = leads.max(lead_of(&id));
                dropped_tabs += tabs_of(&id);
            }
            let position = Point {
                x: (area[0] + area[2]) / 2.0 / 25.4,
                y: (area[1] + area[3]) / 2.0 / 25.4,
            };
            let gid = next_group;
            next_group += 1;
            groups.push(Group {
                id: gid,
                file_id,
                offset: kind.ok_or(ExportError::NothingToCut)?,
                leads: leads > 0.0,
            });
            group_ids.push(gid);
            group_reports.push(GroupReport {
                group_id: gid,
                offset_type: kind.ok_or(ExportError::NothingToCut)?,
                scale: *scale,
                position_in: [position.x, position.y],
                leads: leads > 0.0,
            });
            instances.push(Instance {
                scale: *scale,
                rotation: 0.0,
                position,
            });
        }
        // RSTBB: the file box scaled by instance 0's scale, centred on the origin.
        let s0 = spec.copies[0].0;
        let rst = kurbo::Rect::new(-hw * s0, -hh * s0, hw * s0, hh * s0);
        placements.push(Placement {
            file_id,
            base_bb: file_bb.clone(),
            rst_bb: bbox(rst, Some([rst.x0, rst.y0, rst.x1, rst.y1])),
            instances,
        });
        files.push(FileEntry {
            filename: spec.name.clone(),
            file_id,
            units: LengthUnit::Mm,
            svg_string: svg,
            split_groups: false,
            instances: group_ids.len() as u64,
            group_instances: vec![group_ids],
            file_bb: file_bb.clone(),
        });
        reports.push(FileReport {
            file_id,
            filename: spec.name.clone(),
            cut_contours: cut_names,
            dropped_no_cut_contours: dropped,
            file_bb_in: [fb.x0, fb.y0, fb.x1, fb.y1],
            groups: group_reports,
        });
    }
    if files.is_empty() {
        return Err(ExportError::NothingToCut);
    }
    let any_outside = groups.iter().any(|g| g.offset == OffsetType::Outside);
    let project = Project {
        wam_version: Some("2.4.0".into()),
        file_list: FileList {
            file_id_counter: files.len() as u64,
            files,
        },
        selected_material: SelectedMaterial {
            material: recipe.material,
            quality: recipe.quality,
            abrasive_mode: AbrasiveMode::Speed,
        },
        offsets: groups
            .iter()
            .map(|g| Offset {
                group_id: g.id,
                kind: g.offset,
                distance: WAM_OFFSET_DISTANCE_IN,
                file_id: g.file_id,
            })
            .collect(),
        tabs: groups
            .iter()
            .map(|g| Tab {
                group_id: g.id,
                file_id: Some(g.file_id),
                kind: TabType::None,
                tab_count: 1,
                tab_size: 0.0,
                enable_leads: g.leads,
                cut_type: Some(CutType::Standard),
                scored_tab_speed_multiplier: Some(3.0),
                tab_locations: vec![vec![]],
            })
            .collect(),
        cut_quality: recipe.quality,
        cut_order: Some(CutOrder {
            mode: CutOrderMode::Automatic,
            groups: groups
                .iter()
                .map(|g| CutOrderGroup {
                    path_group_id: g.id,
                    segments: vec![()],
                })
                .collect(),
        }),
        display_unit: Some(LengthUnit::In),
        placements,
    };
    let mut unverified = vec![
        "bed position: assumes WAM bed origin top-left, y down, inches",
        "RSTBB centred on the exact geometry bbox centre (WAM's centre differs by up to 3.4e-5 in)",
        "group ids are numbered 0.. in file then copy order; WAM's own ids follow creation order",
    ];
    if recipe.user_defined {
        unverified.push("custom material: eco-mode rates are written exactly as imported, or as speed feed x 0.0947 for a recipe WAM did not write");
    } else {
        unverified.push(
            "material uId null with userDef true (WAM-native user materials carry a server uId)",
        );
        unverified.push("eco-mode feeds = speed feed x 0.0947 (ratio from the one WAM Pro sample)");
    }
    if project.selected_material.material.u_id.is_none() && recipe.user_defined {
        unverified.push("custom material has no uId (WAM assigns one on its server)");
    }
    if any_outside {
        unverified.push("offset type Outside has not appeared in a WAM-saved sample");
    }
    if groups.iter().any(|g| g.offset == OffsetType::Centerline) {
        unverified.push("Centerline offset type is seen in a WAM-saved sample (E) but not written by this exporter before");
    }
    if dropped_tabs > 0 {
        unverified.push("openWAM tabs are not written; WAM cuts these parts without tabs");
    }
    let report = ExportReport {
        verified_project_sha256: sha,
        files: reports,
        omitted_files: omitted,
        material_user_defined: recipe.user_defined,
        dropped_tabs,
        feed: recipe.feed,
        feed_unit: recipe.rate_unit.into(),
        pierce_s: recipe.pierce,
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
