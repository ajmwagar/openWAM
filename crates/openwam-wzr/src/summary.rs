//! Normalised summary of a `.wzr` for `openwam-wzr read`.

use crate::bbox::wam_file_bb;
use crate::model::{Project, Quality};
use serde_json::{Value, json};

fn rate(p: &Project) -> &crate::model::MathUnit {
    let m = &p.selected_material.material;
    let r = match p.selected_material.abrasive_mode {
        crate::model::AbrasiveMode::Speed => &m.speed_mode,
        crate::model::AbrasiveMode::Eco => &m.eco_mode,
    };
    match p.cut_quality {
        Quality::Rough => &r.rough_rate,
        Quality::Medium => &r.medium_rate,
        Quality::Fine => &r.fine_rate,
    }
}

pub fn summarize(p: &Project) -> Value {
    let files: Vec<Value> = p
        .file_list
        .files
        .iter()
        .map(|f| {
            let recomputed = wam_file_bb(&f.svg_string);
            let stored = [
                f.file_bb.min_x,
                f.file_bb.min_y,
                f.file_bb.max_x,
                f.file_bb.max_y,
            ];
            let (re, delta) = match &recomputed {
                Ok(r) => {
                    let got = [r.x0, r.y0, r.x1, r.y1];
                    let d = got
                        .iter()
                        .zip(stored)
                        .map(|(g, s)| s.map_or(f64::INFINITY, |s| (g - s).abs()))
                        .fold(0.0, f64::max);
                    (json!(got), json!(d))
                }
                Err(e) => (json!({"error": e.to_string()}), Value::Null),
            };
            json!({
                "fileID": f.file_id, "filename": f.filename, "units": f.units,
                "groups": f.group_instances[0], "instances": f.instances,
                "fileBB_in": stored, "fileBB_recomputed_in": re, "fileBB_max_abs_delta_in": delta,
                "svg_bytes": f.svg_string.len(),
            })
        })
        .collect();
    let placements: Vec<Value> = p
        .placements
        .iter()
        .map(|pl| {
            let instances: Vec<Value> = pl
                .instances
                .iter()
                .map(|i| {
                    let identity = i.scale == 1.0 && i.rotation == 0.0;
                    json!({"scale": i.scale, "rotation_deg": i.rotation, "position_in": [i.position.x, i.position.y],
                           "rstbb": if identity { "verified" } else { "unverified" }})
                })
                .collect();
            json!({"fileID": pl.file_id, "instances": instances})
        })
        .collect();
    let m = &p.selected_material.material;
    let r = rate(p);
    json!({
        "schema": "openwam-wzr.summary.v1",
        "wamVersion": p.wam_version,
        "files": files,
        "material": {"name": m.name, "category": m.category, "userDef": m.user_def, "uId": m.u_id,
                     "machineType": m.machine_type, "thickness": [m.thickness.value, m.thickness.unit],
                     "quality": p.cut_quality, "abrasiveMode": p.selected_material.abrasive_mode,
                     "feed": [r.value, r.unit], "pierce_s": m.pierce_time.value,
                     "leadDist": [m.default_lead_dist.value, m.default_lead_dist.unit]},
        "offsets": p.offsets.iter().map(|o| json!({"groupID": o.group_id, "type": o.kind, "distance_in": o.distance})).collect::<Vec<_>>(),
        "tabs": p.tabs.iter().map(|t| json!({"groupID": t.group_id, "type": t.kind, "tabCount": t.tab_count,
            "locations": t.tab_locations.iter().map(Vec::len).sum::<usize>(), "enableLeads": t.enable_leads})).collect::<Vec<_>>(),
        "placements": placements,
        "machine_output_enabled": false,
        "hardware_qualified": false,
    })
}
