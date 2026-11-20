//! Cross-reference checks every WAM-written sample satisfies.

use crate::json::WzrError;
use crate::model::{BBox, Project};
use std::collections::{BTreeMap, BTreeSet};

fn bad(msg: impl Into<String>) -> WzrError {
    WzrError::Inconsistent(msg.into())
}

fn finite_box(b: &BBox, what: &str) -> Result<(), WzrError> {
    let originals = [
        b.original_min_x,
        b.original_min_y,
        b.original_max_x,
        b.original_max_y,
    ];
    let present = originals.iter().filter(|v| v.is_some()).count();
    if present != 0 && present != 4 {
        return Err(bad(format!(
            "{what}: original* fields must be all present or all absent"
        )));
    }
    Ok(())
}

pub(crate) fn consistency(p: &Project) -> Result<(), WzrError> {
    let mut group_owner: BTreeMap<u64, u64> = BTreeMap::new();
    let mut file_ids = BTreeSet::new();
    for f in &p.file_list.files {
        if !file_ids.insert(f.file_id) {
            return Err(bad(format!("duplicate fileID {}", f.file_id)));
        }
        if f.file_id >= p.file_list.file_id_counter {
            return Err(bad(format!(
                "fileID {} not below fileIDCounter {}",
                f.file_id, p.file_list.file_id_counter
            )));
        }
        if f.split_groups {
            return Err(bad("splitGroups:true is not covered by any sample"));
        }
        if f.group_instances.len() != 1 {
            return Err(bad(format!(
                "file {}: expected one groupInstances list without splitGroups",
                f.file_id
            )));
        }
        if f.group_instances[0].len() as u64 != f.instances {
            return Err(bad(format!(
                "file {}: instances {} but {} group IDs",
                f.file_id,
                f.instances,
                f.group_instances[0].len()
            )));
        }
        for g in &f.group_instances[0] {
            if group_owner.insert(*g, f.file_id).is_some() {
                return Err(bad(format!("group {g} listed twice")));
            }
        }
        finite_box(&f.file_bb, "fileBB")?;
    }
    let groups: BTreeSet<u64> = group_owner.keys().copied().collect();

    let check_set = |what: &str, ids: Vec<(u64, Option<u64>)>| -> Result<(), WzrError> {
        let mut seen = BTreeSet::new();
        for (g, file) in ids {
            let owner = group_owner
                .get(&g)
                .ok_or_else(|| bad(format!("{what}: unknown group {g}")))?;
            if let Some(file) = file
                && file != *owner
            {
                return Err(bad(format!(
                    "{what}: group {g} names file {file}, owned by {owner}"
                )));
            }
            if !seen.insert(g) {
                return Err(bad(format!("{what}: group {g} twice")));
            }
        }
        if seen != groups {
            return Err(bad(format!("{what}: does not cover every group")));
        }
        Ok(())
    };
    check_set(
        "offsets",
        p.offsets
            .iter()
            .map(|o| (o.group_id, Some(o.file_id)))
            .collect(),
    )?;
    check_set(
        "tabs",
        p.tabs.iter().map(|t| (t.group_id, t.file_id)).collect(),
    )?;
    if let Some(order) = &p.cut_order {
        check_set(
            "cutOrder",
            order
                .groups
                .iter()
                .map(|g| (g.path_group_id, None))
                .collect(),
        )?;
    }
    if p.selected_material.quality != p.cut_quality {
        return Err(bad("selectedMaterial.quality differs from cutQuality"));
    }
    let mut placed = BTreeSet::new();
    for pl in &p.placements {
        let f = p
            .file_list
            .files
            .iter()
            .find(|f| f.file_id == pl.file_id)
            .ok_or_else(|| bad(format!("placement for unknown file {}", pl.file_id)))?;
        if !placed.insert(pl.file_id) {
            return Err(bad(format!("two placements for file {}", pl.file_id)));
        }
        if pl.instances.len() as u64 != f.instances {
            return Err(bad(format!(
                "file {}: {} instances placed, {} declared",
                f.file_id,
                pl.instances.len(),
                f.instances
            )));
        }
        finite_box(&pl.base_bb, "baseBB")?;
        finite_box(&pl.rst_bb, "RSTBB")?;
    }
    if placed != file_ids {
        return Err(bad("every file needs exactly one placement"));
    }
    Ok(())
}

/// serde_json writes non-finite floats as `null`; WAM files never contain them,
/// so every float is checked before writing.
pub(crate) fn all_finite(p: &Project) -> bool {
    use crate::model::{Lead, MathUnit, PathPoint, Rates};
    fn bb(b: &BBox) -> bool {
        [
            b.max_x,
            b.max_y,
            b.min_x,
            b.min_y,
            b.width,
            b.height,
            b.original_min_x,
            b.original_min_y,
            b.original_max_x,
            b.original_max_y,
        ]
        .iter()
        .flatten()
        .all(|v| v.is_finite())
    }
    fn mu(m: &MathUnit) -> bool {
        m.value.is_finite()
    }
    fn rates(r: &Rates) -> bool {
        [
            &r.abrasive_rate,
            &r.rough_rate,
            &r.medium_rate,
            &r.fine_rate,
        ]
        .into_iter()
        .all(mu)
    }
    fn lead(l: &Lead) -> bool {
        l.points.iter().all(point)
            && bb(&l.base_bb)
            && l.center.x.is_finite()
            && l.center.y.is_finite()
    }
    fn point(p: &PathPoint) -> bool {
        [p.x, p.y, p.path_distance, p.scored_tab_speed_multiplier]
            .iter()
            .all(|v| v.is_finite())
            && p.lead.as_deref().is_none_or(lead)
    }
    let m = &p.selected_material.material;
    p.file_list.files.iter().all(|f| bb(&f.file_bb))
        && [
            &m.thickness,
            &m.pierce_time,
            &m.default_tab_width,
            &m.default_lead_dist,
        ]
        .into_iter()
        .all(mu)
        && rates(&m.speed_mode)
        && rates(&m.eco_mode)
        && p.offsets.iter().all(|o| o.distance.is_finite())
        && p.tabs.iter().all(|t| {
            t.tab_size.is_finite()
                && t.scored_tab_speed_multiplier.is_none_or(f64::is_finite)
                && t.tab_locations.iter().flatten().all(|l| {
                    point(&l.tab_start)
                        && point(&l.tab_end)
                        && l.tab_size.is_finite()
                        && l.scored_tab_speed_multiplier.is_finite()
                })
        })
        && p.placements.iter().all(|pl| {
            bb(&pl.base_bb)
                && bb(&pl.rst_bb)
                && pl.instances.iter().all(|i| {
                    [i.scale, i.rotation, i.position.x, i.position.y]
                        .iter()
                        .all(|v| v.is_finite())
                })
        })
}
