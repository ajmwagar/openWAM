//! Read-side views of a `.wzr` for clients: the recipe in openWAM's request
//! shape and the designs with their placed copies. Pure reads; nothing here
//! writes a file or produces machine output.

use crate::model::{
    AbrasiveMode, LengthUnit, MachineType, MathUnit, Offset, OffsetType, Project, Quality, TabType,
};
use serde_json::{Value, json};

#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error("material: {0}")]
    Material(String),
    #[error(transparent)]
    Svg(#[from] crate::bbox::BBoxError),
}

fn convert(v: &MathUnit, want: LengthUnit, per_min: bool) -> Result<f64, OpenError> {
    let (here, rate) = match v.unit.as_str() {
        "in" => (LengthUnit::In, false),
        "mm" => (LengthUnit::Mm, false),
        "in / min" => (LengthUnit::In, true),
        "mm / min" => (LengthUnit::Mm, true),
        other => return Err(OpenError::Material(format!("unsupported unit {other:?}"))),
    };
    if rate != per_min {
        return Err(OpenError::Material(format!("unexpected unit {:?}", v.unit)));
    }
    Ok(match (here, want) {
        (LengthUnit::In, LengthUnit::Mm) => v.value * 25.4,
        (LengthUnit::Mm, LengthUnit::In) => v.value / 25.4,
        _ => v.value,
    })
}

fn seconds(v: &MathUnit) -> Result<f64, OpenError> {
    if v.unit == "s" {
        Ok(v.value)
    } else {
        Err(OpenError::Material(format!("pierceTime unit {:?}", v.unit)))
    }
}

/// The project's material as an openWAM `settings.user_material` recipe.
///
/// `feed` is the speed-mode rate of the file's cut quality, which is what the
/// openWAM plan uses; the eco rates and the abrasive rate ride along in `wam` so
/// the recipe can be written back unchanged. `origin` names the file it came from.
pub fn material_recipe(p: &Project, origin: &str) -> Result<Value, OpenError> {
    let m = &p.selected_material.material;
    let units = m.units;
    let speed = &m.speed_mode;
    let eco = &m.eco_mode;
    let rates = |r: &crate::model::Rates| -> Result<[f64; 3], OpenError> {
        Ok([
            convert(&r.rough_rate, units, true)?,
            convert(&r.medium_rate, units, true)?,
            convert(&r.fine_rate, units, true)?,
        ])
    };
    let (speed_rates, eco_rates) = (rates(speed)?, rates(eco)?);
    let (cut_rate, idx) = match p.selected_material.quality {
        Quality::Rough => ("coarse", 0),
        Quality::Medium => ("medium", 1),
        Quality::Fine => ("fine", 2),
    };
    if speed.abrasive_rate.unit != "g / min" {
        return Err(OpenError::Material("abrasiveRate unit".into()));
    }
    Ok(json!({
        "user_defined": true,
        "name": m.name,
        "category": m.category,
        "model": match m.machine_type { MachineType::Pro => "pro", MachineType::Desktop => "desktop" },
        "units": match units { LengthUnit::In => "imperial", LengthUnit::Mm => "metric" },
        "thickness": convert(&m.thickness, units, false)?,
        "feed": speed_rates[idx],
        "pierce_seconds": seconds(&m.pierce_time)?,
        "lead_length": convert(&m.default_lead_dist, units, false)?,
        "tab_size": convert(&m.default_tab_width, units, false)?,
        "cut_rate": cut_rate,
        "origin": format!("{origin} (WAM 2.4 material, uId {})", m.u_id.map_or("none".to_owned(), |u| u.to_string())),
        "wam": {"uid": m.u_id, "speed_rates": speed_rates, "eco_rates": eco_rates,
                "abrasive_g_min": speed.abrasive_rate.value, "max_tab": m.max_tab},
    }))
}

fn offset_of(p: &Project, group: u64) -> Option<&Offset> {
    p.offsets.iter().find(|o| o.group_id == group)
}

/// Designs with their placed copies, ready to put on a bed: each file's
/// paths-only SVG (what WAM imports) and one entry per group.
pub fn designs(p: &Project) -> Result<Vec<Value>, OpenError> {
    let mut out = Vec::new();
    for f in &p.file_list.files {
        let paths = crate::svg::paths_only(&f.svg_string)?;
        let placement = p.placements.iter().find(|pl| pl.file_id == f.file_id);
        let instances: Vec<Value> = f.group_instances[0]
            .iter()
            .enumerate()
            .map(|(i, g)| {
                let inst = placement.and_then(|pl| pl.instances.get(i));
                let offset = offset_of(p, *g);
                let tab = p.tabs.iter().find(|t| t.group_id == *g);
                json!({
                    "group_id": g,
                    "scale": inst.map(|v| v.scale),
                    "rotation_deg": inst.map(|v| v.rotation),
                    "position_in": inst.map(|v| [v.position.x, v.position.y]),
                    "offset_type": offset.map(|o| match o.kind {
                        OffsetType::Inside => "inside",
                        OffsetType::Outside => "outside",
                        OffsetType::Centerline => "centerline",
                    }),
                    "tabs": tab.map(|t| match t.kind { TabType::Automatic => "automatic", TabType::None => "none" }),
                    "enable_leads": tab.map(|t| t.enable_leads),
                })
            })
            .collect();
        out.push(json!({
            "file_id": f.file_id,
            "filename": f.filename,
            "units": f.units,
            "svg": paths.svg,
            "paths": paths.paths,
            "ignored_shapes": paths.ignored_shapes,
            "file_bb_in": [f.file_bb.min_x, f.file_bb.min_y, f.file_bb.max_x, f.file_bb.max_y],
            "instances": instances,
        }));
    }
    Ok(out)
}

pub fn abrasive_mode(p: &Project) -> &'static str {
    match p.selected_material.abrasive_mode {
        AbrasiveMode::Speed => "speed",
        AbrasiveMode::Eco => "eco",
    }
}
