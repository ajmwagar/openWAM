use kurbo::{CubicBez, ParamCurve, Point};

#[test]
fn layout_viewports_are_explicit_metadata_and_frozen_layers_fail() {
    let viewport = "0\nVIEWPORT\n67\n1\n10\n100\n20\n100\n40\n200\n41\n200\n";
    let bytes = dxf(&format!("{}{viewport}", line()), 4);
    let sketch = dxf_intake::inspect_dxf(&bytes, 0.02).unwrap();
    assert_eq!(sketch.contours.len(), 1);
    assert!(
        sketch
            .diagnostics
            .iter()
            .any(|v| v.contains("1 paper-space VIEWPORT"))
    );
    let model = dxf(
        &format!("{}{}", line(), viewport.replace("67\n1", "67\n0")),
        4,
    );
    assert!(dxf_intake::inspect_dxf(&model, 0.02).is_err());
    let text = String::from_utf8(dxf(line(), 4)).unwrap();
    let frozen=text.replace("0\nSECTION\n2\nENTITIES", "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n0\nLAYER\n2\n0\n70\n1\n0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES");
    assert!(dxf_intake::inspect_dxf(frozen.as_bytes(), 0.02).is_err());
}
use reversal_sketch_intake::{
    bridge::{self, Selection, SourceFormat},
    cutting::{self, Model, Units},
    dxf_intake, inspect_svg,
};
use transmog_core::ir::{Feature, SketchSegment};

fn svg() -> &'static [u8] {
    br#"<svg xmlns="http://www.w3.org/2000/svg" width="30mm" height="30mm" viewBox="0 0 30 30"><path id="curve" d="M0 0 C0 20 20 20 20 0"/></svg>"#
}
fn selection() -> Selection<'static> {
    Selection {
        model: Model::Pro,
        material: "Aluminum",
        units: Units::Metric,
        thickness: 3.0,
    }
}
fn dxf(entities: &str, units: i32) -> Vec<u8> {
    format!("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1015\n9\n$INSUNITS\n70\n{units}\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n").into_bytes()
}
fn line() -> &'static str {
    "0\nLINE\n10\n0\n20\n0\n30\n0\n11\n1\n21\n-1\n31\n0\n"
}

#[test]
fn bezier_adapter_meets_measured_tolerance_and_does_not_mislabel_splines() {
    let sketch = inspect_svg(svg()).unwrap();
    let coarse = bridge::to_transmog(&sketch, 0.1, [0.0, 0.0]).unwrap();
    let fine = bridge::to_transmog(&sketch, 0.01, [0.0, 0.0]).unwrap();
    assert!(fine.max_bezier_chord_bound_mm <= 0.01);
    let Feature::Sketch { profile, .. } = &fine.features[0] else {
        panic!()
    };
    let Feature::Sketch {
        profile: coarse, ..
    } = &coarse.features[0]
    else {
        panic!()
    };
    assert!(profile.len() > coarse.len());
    let original = CubicBez::new((0.0, 0.0), (0.0, -20.0), (20.0, -20.0), (20.0, 0.0));
    for i in 0..=1000 {
        let p = original.eval(i as f64 / 1000.0);
        let distance = profile
            .iter()
            .map(|s| {
                let SketchSegment::Line { start, end } = s else {
                    panic!("only Transmog line segments should be emitted")
                };
                let a = Point::new(start.x, start.y);
                let b = Point::new(end.x, end.y);
                let d = b - a;
                let t = ((p - a).dot(d) / d.hypot2()).clamp(0.0, 1.0);
                p.distance(a + d * t)
            })
            .fold(f64::INFINITY, f64::min);
        assert!(distance <= 0.01001, "distance={distance}");
    }
}

#[test]
fn svg_transmog_swarf_preserves_holes_mirroring_closure_and_source_identity() {
    let input=br#"<svg xmlns="http://www.w3.org/2000/svg" width="30mm" height="30mm" viewBox="0 0 30 30"><g transform="translate(25 0) scale(-1 1)"><path id="outer" d="M0 0H20V20H0Z"/><circle id="hole" cx="10" cy="10" r="3"/></g></svg>"#;
    let report = bridge::integrate_source(
        input,
        SourceFormat::Svg,
        &cutting::bundled().unwrap(),
        selection(),
        0.02,
        [0.0, 0.0],
    )
    .unwrap();
    assert_eq!(report.transmog.features.len(), 2);
    assert!(report.swarf.request.contours.iter().all(|c| c.closed));
    assert_eq!(report.planning.raw_contour_pierces, 2);
    assert_eq!(report.swarf.request.feed_mm_min, 108.0);
    assert_eq!(report.swarf.request.nominal_kerf_mm, 1.1);
    assert!(!report.machine_output_enabled && !report.swarf.machine_output_enabled);
    let swarf_waterjet::Operation::Rapid { to_mm } = report.swarf.operations[0] else {
        panic!()
    };
    assert!((to_mm[0] - 25.0).abs() < 0.00001 && to_mm[1] == 0.0);
}

#[test]
fn original_recomputation_rejects_forged_operations_and_geometry() {
    let catalog = cutting::bundled().unwrap();
    let r = bridge::integrate_source(
        svg(),
        SourceFormat::Svg,
        &catalog,
        selection(),
        0.02,
        [1.0, 2.0],
    )
    .unwrap();
    let bytes = serde_json::to_vec_pretty(&r).unwrap();
    assert_eq!(
        bridge::verify_source(svg(), &bytes, &catalog).unwrap()["verified"],
        true
    );
    let mut forged = serde_json::to_value(&r).unwrap();
    forged["swarf"]["operations"][0]["to_mm"] = serde_json::json!([300.0, -300.0]);
    assert!(bridge::verify_source(svg(), &serde_json::to_vec(&forged).unwrap(), &catalog).is_err());
    let mut forged = serde_json::to_value(&r).unwrap();
    forged["machine_output_enabled"] = true.into();
    assert!(bridge::verify_source(svg(), &serde_json::to_vec(&forged).unwrap(), &catalog).is_err());
    let drift = String::from_utf8(svg().to_vec())
        .unwrap()
        .replace("C0 20", "C0 21");
    assert!(bridge::verify_source(drift.as_bytes(), &bytes, &catalog).is_err());
}

#[test]
fn dxf_units_are_explicit_and_unknown_partial_entities_fail() {
    let metric = dxf_intake::inspect_dxf(&dxf(line(), 4), 0.02).unwrap();
    let imperial = dxf_intake::inspect_dxf(&dxf(line(), 1), 0.02).unwrap();
    let reversal_sketch_intake::Segment::Line { to } = metric.contours[0].segments[1] else {
        panic!()
    };
    assert_eq!(to, [1.0, 1.0]);
    let reversal_sketch_intake::Segment::Line { to } = imperial.contours[0].segments[1] else {
        panic!()
    };
    assert_eq!(to, [25.4, 25.4]);
    assert!(dxf_intake::inspect_dxf(&dxf(line(), 0), 0.02).is_err());
    assert!(
        dxf_intake::inspect_dxf(&dxf(&format!("{}0\nMYSTERY\n10\n0\n", line()), 4), 0.02).is_err()
    );
    assert!(dxf_intake::inspect_dxf(&dxf(&line().replace("31\n0", "31\n1"), 4), 0.02).is_err());
    assert!(dxf_intake::inspect_dxf(&dxf(line(), 4)[..20], 0.02).is_err());
}

#[test]
fn dxf_circles_and_explicit_closed_polylines_integrate_with_bounded_curves() {
    let circle = "0\nCIRCLE\n10\n10\n20\n-10\n30\n0\n40\n3\n";
    let polyline = "0\nLWPOLYLINE\n90\n4\n70\n1\n10\n0\n20\n0\n10\n20\n20\n0\n10\n20\n20\n-20\n10\n0\n20\n-20\n";
    let bytes = dxf(&format!("{circle}{polyline}"), 4);
    let r = bridge::integrate_source(
        &bytes,
        SourceFormat::Dxf,
        &cutting::bundled().unwrap(),
        selection(),
        0.02,
        [0.0, 0.0],
    )
    .unwrap();
    assert_eq!(r.transmog.features.len(), 2);
    assert!(r.swarf.request.contours.iter().all(|c| c.closed));
    assert!((r.swarf.raw_centerline_length_mm - (80.0 + 6.0 * std::f64::consts::PI)).abs() < 0.13);
    let serialized = serde_json::to_vec(&r).unwrap();
    assert!(bridge::verify_source(&bytes, &serialized, &cutting::bundled().unwrap()).is_ok());
}

#[test]
fn endpoint_gaps_stay_separate_and_stock_placement_is_explicit() {
    let bytes = dxf(
        &format!("{}{}", line(), line().replace("10\n0", "10\n1.001")),
        4,
    );
    let r = bridge::integrate_source(
        &bytes,
        SourceFormat::Dxf,
        &cutting::bundled().unwrap(),
        selection(),
        0.02,
        [1.0, 1.0],
    )
    .unwrap();
    assert_eq!(r.swarf.request.contours.len(), 2);
    assert!(r.swarf.request.contours.iter().all(|c| !c.closed));
    assert!(r.swarf.blockers.iter().any(|v| v.contains("Open contours")));
    assert!(
        bridge::integrate_source(
            &bytes,
            SourceFormat::Dxf,
            &cutting::bundled().unwrap(),
            selection(),
            0.02,
            [-10.0, 0.0]
        )
        .is_err()
    );
    assert!(bridge::to_transmog(&inspect_svg(svg()).unwrap(), 0.0, [0.0, 0.0]).is_err());
}
