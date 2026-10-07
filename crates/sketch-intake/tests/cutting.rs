use reversal_sketch_intake::{
    cutting::{self, Model, Units},
    inspect_svg,
};
use std::process::Command;

fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-5, "{a} != {b}");
}
fn rectangle() -> String {
    r#"<svg xmlns="http://www.w3.org/2000/svg" width="30mm" height="20mm" viewBox="0 0 30 20"><path d="M0 0H20V10H0Z"/></svg>"#.into()
}

#[test]
fn published_metric_and_imperial_points_are_separate_and_model_scoped() {
    let c = cutting::bundled().unwrap();
    let metric = c
        .select(Model::Desktop, "Aluminum", Units::Metric, 3.0)
        .unwrap();
    let imperial = c
        .select(Model::Desktop, "Aluminum", Units::Imperial, 0.125)
        .unwrap();
    let pro = c
        .select(Model::Pro, "Aluminum", Units::Metric, 3.0)
        .unwrap();
    near(metric.cut.as_ref().unwrap().feed, 45.0);
    near(metric.cut.as_ref().unwrap().pierce_seconds, 18.0);
    near(imperial.cut.as_ref().unwrap().feed, 1.7);
    near(imperial.cut.as_ref().unwrap().pierce_seconds, 20.0);
    near(pro.cut.as_ref().unwrap().feed, 108.0);
    assert!((1.7 * 25.4 - 45.0_f64).abs() > 1.0);
}

#[test]
fn snapshot_covers_each_source_row_in_all_model_unit_variants() {
    let c = cutting::bundled().unwrap();
    let rows: std::collections::BTreeSet<_> = c.rows.iter().map(|r| r.source_row).collect();
    assert_eq!(c.rows.len(), rows.len() * 4);
    assert!(c.rows.iter().any(|r| r.cut.is_none()));
    assert!(c.rows.iter().any(|r| r.maximum_marked));
    assert!(c.rows.iter().any(|r| !r.warnings.is_empty()));
}

#[test]
fn missing_rows_alloy_aliases_and_source_anomalies_are_refused() {
    let c = cutting::bundled().unwrap();
    for (m, name, u, t) in [
        (Model::Desktop, "Aluminum", Units::Metric, 19.0),
        (Model::Desktop, "Aluminum", Units::Metric, 3.175),
        (Model::Pro, "Aluminum 6061", Units::Metric, 3.0),
        (Model::Pro, "Brass", Units::Imperial, 0.0125),
        (Model::Pro, "Brass", Units::Metric, 3.0),
        (Model::Pro, "Aluminum", Units::Metric, f64::NAN),
    ] {
        assert!(c.select(m, name, u, t).is_err());
    }
    let mut damaged = c.clone();
    damaged.rows.pop();
    assert!(damaged.validate().is_err());
    let mut damaged = c.clone();
    for r in &mut damaged.rows {
        r.warnings.clear();
    }
    assert!(damaged.validate().is_err());
    let mut damaged = c;
    damaged.rows.push(damaged.rows[0].clone());
    assert!(damaged.validate().is_err());
}

#[test]
fn svg_to_recipe_estimate_binds_units_geometry_and_source() {
    let svg = rectangle();
    let c = cutting::bundled().unwrap();
    let job = cutting::plan(
        inspect_svg(svg.as_bytes()).unwrap(),
        &c,
        Model::Desktop,
        "Aluminum",
        Units::Metric,
        3.0,
    )
    .unwrap();
    near(job.raw_centerline_length_mm, 60.0);
    near(job.nominal_cut_and_pierce_seconds, 98.0);
    near(job.nominal_abrasive_kg, 0.245);
    near(job.nominal_kerf_mm, 1.1);
    near(job.nominal_side_offset_mm, 0.55);
    near(job.feed_ipm, 45.0 / 25.4);
    assert_eq!(job.raw_contour_pierces, 1);
    assert_eq!(job.tables_canonical_sha256.len(), 64);
    assert_eq!(job.sketch.source_sha256.len(), 64);
    assert!(!job.machine_output_enabled && !job.blockers.is_empty());
    let again = cutting::plan(
        inspect_svg(svg.as_bytes()).unwrap(),
        &c,
        Model::Desktop,
        "Aluminum",
        Units::Metric,
        3.0,
    )
    .unwrap();
    assert_eq!(
        serde_json::to_vec(&job).unwrap(),
        serde_json::to_vec(&again).unwrap()
    );
}

#[test]
fn independent_verification_rejects_forged_estimates_and_input_drift() {
    let svg = rectangle();
    let c = cutting::bundled().unwrap();
    let job = cutting::plan(
        inspect_svg(svg.as_bytes()).unwrap(),
        &c,
        Model::Desktop,
        "Aluminum",
        Units::Metric,
        3.0,
    )
    .unwrap();
    let bytes = serde_json::to_vec_pretty(&job).unwrap();
    assert_eq!(
        cutting::verify_job(svg.as_bytes(), &bytes, &c).unwrap()["verified"],
        true
    );
    let mut forged = serde_json::to_value(&job).unwrap();
    forged["feed_mm_min"] = serde_json::json!(1000.0);
    assert!(
        cutting::verify_job(svg.as_bytes(), &serde_json::to_vec(&forged).unwrap(), &c).is_err()
    );
    let drift = svg.replace("H20", "H21");
    assert!(cutting::verify_job(drift.as_bytes(), &bytes, &c).is_err());
    let mut drift = c.clone();
    drift
        .rows
        .iter_mut()
        .find(|r| {
            r.model == Model::Desktop
                && r.units == Units::Metric
                && r.material == "Aluminum"
                && r.thickness == 3.0
        })
        .unwrap()
        .cut
        .as_mut()
        .unwrap()
        .feed = 44.0;
    assert!(cutting::verify_job(svg.as_bytes(), &bytes, &drift).is_err());
}

#[test]
fn captured_public_calculator_matches_published_display_settings() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../wazer/calculator-crosscheck.json")).unwrap();
    let c = cutting::bundled().unwrap();
    for m in fixture["models"].as_array().unwrap() {
        let model: Model = serde_json::from_value(m["model"].clone()).unwrap();
        let r = c
            .select(
                model,
                fixture["material"].as_str().unwrap(),
                Units::Imperial,
                fixture["thickness_in"].as_f64().unwrap(),
            )
            .unwrap();
        near(
            r.cut.as_ref().unwrap().feed,
            m["display_feed_ipm"].as_f64().unwrap(),
        );
        near(
            r.cut.as_ref().unwrap().pierce_seconds,
            m["pierce_seconds"].as_f64().unwrap(),
        );
    }
}

#[test]
fn planning_marks_out_of_area_and_unresolved_open_geometry() {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="1000mm" height="1000mm" viewBox="0 0 1000 1000"><path d="M0 0L600 0"/></svg>"#;
    let job = cutting::plan(
        inspect_svg(svg.as_bytes()).unwrap(),
        &cutting::bundled().unwrap(),
        Model::Pro,
        "Aluminum",
        Units::Metric,
        3.0,
    )
    .unwrap();
    assert!(job.blockers.iter().any(|s| s.contains("cutting area")));
    assert!(job.blockers.iter().any(|s| s.contains("Open contours")));
}

#[test]
fn long_nominal_jobs_report_refill_limits() {
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="450mm" height="300mm" viewBox="0 0 450 300"><path d="M0 0H450V300H0Z"/></svg>"#;
    let job = cutting::plan(
        inspect_svg(svg.as_bytes()).unwrap(),
        &cutting::bundled().unwrap(),
        Model::Desktop,
        "Aluminum",
        Units::Metric,
        13.0,
    )
    .unwrap();
    assert!(job.nominal_cut_and_pierce_seconds / 60.0 > job.machine_limits.continuous_cut_minutes);
    assert!(job.nominal_abrasive_kg > job.machine_limits.hopper_capacity_kg);
    assert!(job.blockers.iter().any(|s| s.contains("refill planning")));
}

#[test]
fn table_import_fails_loudly_on_missing_or_unexpected_sources() {
    assert!(
        cutting::import_published(b"<html>Denied</html>", b"<html></html>", "2026-10-06").is_err()
    );
    assert!(cutting::import_published(b"", b"", "yesterday").is_err());
    assert!(
        cutting::import_published(&vec![b' '; 2 * 1024 * 1024 + 1], b"", "2026-10-06").is_err()
    );
}

#[test]
fn html_import_preserves_unit_points_and_detects_column_and_spec_drift() {
    let material = r#"<div class="materials-table__list-item-table"><table>
      <tr><td class="materials-table__header-product-cell">Wazer Pro</td><td class="materials-table__header-product-cell">Wazer Desktop</td></tr>
      <tr><th class="materials-table__cell-material">Test material</th>
      <td class="materials-table__cell-thickness"><span class="unit-imperial">0.125″</span><span class="unit-metric">3 mm</span></td>
      <td class="materials-table__cell-cut-rate"><span class="unit-imperial">4.0 ipm</span><span class="unit-metric">108 mmpm</span></td>
      <td class="materials-table__cell-cut-speed"><span class="unit-imperial">7 sec</span><span class="unit-metric">6 sec</span></td>
      <td class="materials-table__cell-cut-rate"><span class="unit-imperial">1.7 ipm</span><span class="unit-metric">45 mmpm</span></td>
      <td class="materials-table__cell-cut-speed"><span class="unit-imperial">1m 20s</span><span class="unit-metric">1m 18s</span></td>
      </tr></table></div>"#;
    let specs = r#"<figure class="is-style-specs"><table>
      <tr><th>Kerf (width of cut)</th><td>0.044 in<br>1.1 mm</td><td>0.044 in<br>1.1 mm</td></tr>
      <tr><th>Cutting Area (D x W)</th><td>12 x 18 in<br>305 x 460 mm</td><td>12 x 18 in<br>305 x 460 mm</td></tr>
      <tr><th>Gantry Max Speed</th><td>59 IPM<br>1,500 mm/min</td><td>59 IPM<br>1,500 mm/min</td></tr>
      <tr><th>Abrasive Rate</th><td>0.33 lb/min<br>0.15 kg/min</td><td>0.33 lb/min<br>0.15 kg/min</td></tr>
      <tr><th>Abrasive Hopper Capacity</th><td>42 lb<br>19.1 kg</td><td>30 lb<br>13.5 kg</td></tr>
      <tr><th>Continuous Cutting Time</th><td>90 min</td><td>60 min</td></tr>
      </table></figure>"#;
    let c = cutting::import_published(material.as_bytes(), specs.as_bytes(), "2026-10-06").unwrap();
    assert_eq!(c.rows.len(), 4);
    near(
        c.select(Model::Desktop, "Test material", Units::Metric, 3.0)
            .unwrap()
            .cut
            .as_ref()
            .unwrap()
            .pierce_seconds,
        78.0,
    );
    near(
        c.select(Model::Desktop, "Test material", Units::Imperial, 0.125)
            .unwrap()
            .cut
            .as_ref()
            .unwrap()
            .pierce_seconds,
        80.0,
    );
    let drift = material.replace("Wazer Pro", "Wazer Desktop");
    assert!(cutting::import_published(drift.as_bytes(), specs.as_bytes(), "2026-10-06").is_err());
    let drift = format!("{specs}{}", specs.replace("0.044 in", "0.050 in"));
    assert!(
        cutting::import_published(material.as_bytes(), drift.as_bytes(), "2026-10-06").is_err()
    );
}

#[test]
fn actual_cli_emits_job_and_rejects_unsupported_selection() {
    let root = env!("CARGO_MANIFEST_DIR");
    let svg = format!("{root}/examples/plate.svg");
    let out = Command::new(env!("CARGO_BIN_EXE_reversal-sketch-intake"))
        .args(["--plan-wazer", &svg, "desktop", "Aluminum", "3", "metric"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let job: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(job["feed_mm_min"], 45.0);
    assert_eq!(job["machine_output_enabled"], false);
    let out = Command::new(env!("CARGO_BIN_EXE_reversal-sketch-intake"))
        .args(["--plan-wazer", &svg, "desktop", "Aluminum", "19", "metric"])
        .output()
        .unwrap();
    assert!(!out.status.success() && out.stdout.is_empty());
}
