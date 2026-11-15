//! Multi-file export and the material/open reads, from a captured openWAM
//! `plan_cam` artifact (two sources, three groups, a custom Pro recipe).
//! No `openwam` process is needed: the verify receipt is supplied directly.

use openwam_wzr::{
    export::{ExportError, export},
    model::{LengthUnit, MachineType, OffsetType, Quality},
    open, read, write,
};
use serde_json::{Value, json};
use std::path::PathBuf;

const ARTIFACT: &str = include_str!("fixtures/multi-artifact.json");

fn artifact() -> Value {
    serde_json::from_str(ARTIFACT).unwrap()
}

fn receipt(artifact: &Value) -> Value {
    json!({"status": "ok", "machine_output_enabled": false, "result": {"kind": "verification",
        "verification": {"schema": "reversal.openwam-cam-verification.v1", "verified": true,
        "project_sha256": artifact["project_sha256"], "machine_output_enabled": false}}})
}

fn close(got: f64, want: f64) {
    assert!((got - want).abs() < 2e-3, "{got} vs {want}");
}

#[test]
fn bundle_exports_one_file_entry_per_source_with_a_group_per_copy() {
    let a = artifact();
    let (p, report) = export(&a, &receipt(&a), "unused.svg").unwrap();
    let names: Vec<_> = p
        .file_list
        .files
        .iter()
        .map(|f| f.filename.as_str())
        .collect();
    assert_eq!(names, ["plate.svg", "square.svg"]);
    assert_eq!(p.file_list.file_id_counter, 2);
    assert_eq!(p.file_list.files[0].group_instances, vec![vec![0, 1]]);
    assert_eq!(p.file_list.files[1].group_instances, vec![vec![2]]);
    assert_eq!(p.offsets.len(), 3);
    assert!(p.offsets.iter().all(|o| o.kind == OffsetType::Outside));
    assert_eq!(p.offsets[2].file_id, 1);
    assert_eq!(p.tabs.len(), 3);
    assert_eq!(p.cut_order.as_ref().unwrap().groups.len(), 3);
    // The open sketch is dropped from the plate; hole and outline are kept.
    assert_eq!(report.files[0].cut_contours, ["outline", "hole"]);
    assert_eq!(report.files[0].dropped_no_cut_contours, ["open-sketch"]);
    assert!(!p.file_list.files[0].svg_string.contains("open-sketch"));

    // Positions are the placed bounding-box centres, in inches on the bed.
    let plate = &p.placements[0].instances;
    assert_eq!(
        plate.iter().map(|i| i.scale).collect::<Vec<_>>(),
        [1.0, 1.5]
    );
    close(plate[0].position.x, 60.0 / 25.4);
    close(plate[0].position.y, 35.0 / 25.4);
    close(plate[1].position.x, 80.0 / 25.4);
    close(plate[1].position.y, 125.0 / 25.4);
    close(p.placements[1].instances[0].position.x, 170.0 / 25.4);
    close(p.placements[1].instances[0].position.y, 40.0 / 25.4);
    // RSTBB is the file box scaled by instance 0's scale.
    let (fb, rst) = (&p.placements[0].base_bb, &p.placements[0].rst_bb);
    assert!((fb.width.unwrap() - rst.width.unwrap()).abs() < 1e-12);

    // The custom recipe is carried, not a catalog row.
    let m = &p.selected_material.material;
    assert!(m.user_def && m.u_id == Some(257));
    assert_eq!(
        (m.name.as_str(), m.category.as_str()),
        ("Polyurethane", "Plastics & Rubbers")
    );
    assert_eq!(
        (m.units, m.machine_type, m.max_tab),
        (LengthUnit::In, MachineType::Pro, Some(3))
    );
    assert_eq!(m.thickness.value, 0.5);
    assert_eq!(m.speed_mode.fine_rate.value, 23.3);
    assert_eq!(m.eco_mode.medium_rate.value, 2.4799999999999995);
    assert_eq!(m.default_lead_dist.value, 0.15);
    assert_eq!(p.cut_quality, Quality::Fine);
    assert!(report.material_user_defined);
    assert!(!report.machine_output_enabled && !report.hardware_qualified);

    // Writes and reads back byte-identically.
    let bytes = write(&p).unwrap();
    assert_eq!(write(&read(&bytes).unwrap()).unwrap(), bytes);
}

#[test]
fn rotation_and_inconsistent_cut_sets_are_refused_not_guessed() {
    let mut a = artifact();
    a["project"]["integration"]["transmog"]["sources"][0]["instances"][1]["rotation_deg"] =
        json!(90.0);
    assert!(matches!(
        export(&a, &receipt(&a), "x.svg"),
        Err(ExportError::Rotation { copy: 1, .. })
    ));

    let mut a = artifact();
    let paths = a["plan"]["paths"].as_array_mut().unwrap();
    paths.retain(|p| p["source_id"] != "a:hole#1");
    assert!(matches!(
        export(&a, &receipt(&a), "x.svg"),
        Err(ExportError::InconsistentCuts { .. })
    ));

    let mut a = artifact();
    a["plan"]["paths"].as_array_mut().unwrap().clear();
    assert!(matches!(
        export(&a, &receipt(&a), "x.svg"),
        Err(ExportError::NothingToCut)
    ));
}

#[test]
fn a_source_with_nothing_to_cut_is_omitted_and_reported() {
    let mut a = artifact();
    a["plan"]["paths"]
        .as_array_mut()
        .unwrap()
        .retain(|p| !p["source_id"].as_str().unwrap().starts_with("b:"));
    let (p, report) = export(&a, &receipt(&a), "x.svg").unwrap();
    assert_eq!(p.file_list.files.len(), 1);
    assert_eq!(report.omitted_files, ["square.svg"]);
}

#[test]
fn per_group_cutting_path_is_written_per_group() {
    let mut a = artifact();
    // The square (its own group) is cut on the centreline; the plate groups keep their sides.
    for p in a["plan"]["paths"].as_array_mut().unwrap() {
        if p["source_id"] == "b:sq" || p["source_id"] == "b:sq#0" {
            p["side"] = json!("centerline");
        }
    }
    let (p, _) = export(&a, &receipt(&a), "x.svg").unwrap();
    assert_eq!(p.offsets[2].kind, OffsetType::Centerline);
    assert_eq!(p.offsets[0].kind, OffsetType::Outside);
}

fn samples() -> Option<PathBuf> {
    std::env::var_os("OPENWAM_WZR_SAMPLES").map(PathBuf::from)
}

#[test]
fn material_read_returns_the_recipe_from_a_real_wzr_when_present() {
    let Some(dir) = samples() else {
        eprintln!("OPENWAM_WZR_SAMPLES unset; skipping");
        return;
    };
    let Ok(bytes) = std::fs::read(dir.join("Foam Insert.wzr")) else {
        return;
    };
    let p = read(&bytes).unwrap();
    let m = open::material_recipe(&p, "Foam Insert.wzr").unwrap();
    assert_eq!(m["name"], "Polyurethane");
    assert_eq!(m["units"], "imperial");
    assert_eq!(m["thickness"], 0.5);
    assert_eq!(m["feed"], 23.3);
    assert_eq!(m["pierce_seconds"], 4.0);
    assert_eq!(m["cut_rate"], "fine");
    assert_eq!(m["wam"]["uid"], 257);
    let designs = open::designs(&p).unwrap();
    assert_eq!(designs.len(), 1);
    assert_eq!(designs[0]["instances"].as_array().unwrap().len(), 3);
    assert_eq!(designs[0]["ignored_shapes"], 4);
}

#[test]
fn two_file_sample_opens_with_both_designs_when_present() {
    let Some(dir) = samples() else {
        return;
    };
    let Ok(bytes) = std::fs::read(dir.join("wazerjob (1).wzr")) else {
        return;
    };
    let designs = open::designs(&read(&bytes).unwrap()).unwrap();
    let names: Vec<_> = designs
        .iter()
        .map(|d| d["filename"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Ellipse 1.svg", "wam.svg"]);
}
