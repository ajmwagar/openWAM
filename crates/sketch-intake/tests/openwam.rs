use reversal_sketch_intake::{
    bridge::{self, Selection, SourceFormat},
    cutting::{self, Model, Units},
    openwam,
    simulation::Outcome,
};

const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="20mm" height="20mm" viewBox="0 0 20 20"><path id="line" d="M1 1L11 1"/></svg>"#;

fn project(source: &[u8], format: SourceFormat) -> Vec<u8> {
    let report = bridge::integrate_source(
        source,
        format,
        &cutting::bundled().unwrap(),
        Selection {
            model: Model::Pro,
            material: "Aluminum",
            units: Units::Metric,
            thickness: 3.0,
        },
        0.02,
        [5.0, 7.0],
    )
    .unwrap();
    serde_json::to_vec_pretty(&openwam::create(source, &report).unwrap()).unwrap()
}

#[test]
fn portable_project_reopens_and_simulates_without_an_original_file() {
    let bytes = project(SVG, SourceFormat::Svg);
    let catalog = cutting::bundled().unwrap();
    let (retained, rebuilt) = openwam::reopen(&bytes, &catalog).unwrap();
    assert_eq!(retained.source_text.as_bytes(), SVG);
    assert_eq!(rebuilt.transmog.placement_xy_down_mm, [5.0, 7.0]);
    assert_eq!(openwam::check(&bytes, &catalog).unwrap()["verified"], true);
    let replay = openwam::simulate(&bytes, &catalog).unwrap();
    assert_eq!(replay.outcome, Outcome::Completed);
    assert_eq!(replay.completed_operations, 4);
    assert!(!replay.machine_output_enabled && !replay.hardware_qualified);
    assert!((replay.frames.last().unwrap().position_mm[0] - 16.0).abs() < 1e-5);
}

#[test]
fn dxf_source_line_endings_and_explicit_units_survive_json_roundtrip() {
    let dxf = "0\r\nSECTION\r\n2\r\nHEADER\r\n9\r\n$ACADVER\r\n1\r\nAC1015\r\n9\r\n$INSUNITS\r\n70\r\n4\r\n0\r\nENDSEC\r\n0\r\nSECTION\r\n2\r\nENTITIES\r\n0\r\nLINE\r\n10\r\n0\r\n20\r\n0\r\n30\r\n0\r\n11\r\n1\r\n21\r\n-1\r\n31\r\n0\r\n0\r\nENDSEC\r\n0\r\nEOF\r\n";
    let bytes = project(dxf.as_bytes(), SourceFormat::Dxf);
    let (retained, report) = openwam::reopen(&bytes, &cutting::bundled().unwrap()).unwrap();
    assert_eq!(retained.source_text.as_bytes(), dxf.as_bytes());
    assert!((report.swarf.raw_centerline_length_mm - 2_f64.sqrt()).abs() < 1e-9);
}

#[test]
fn changed_source_operations_tables_and_authority_are_refused_before_preview() {
    let original: serde_json::Value =
        serde_json::from_slice(&project(SVG, SourceFormat::Svg)).unwrap();
    let catalog = cutting::bundled().unwrap();
    for (pointer, value) in [
        (
            "/source_text",
            serde_json::json!(std::str::from_utf8(SVG).unwrap().replace("L11", "L12")),
        ),
        (
            "/integration/swarf/operations/2/to_mm/0",
            serde_json::json!(999.0),
        ),
        (
            "/integration/planning/feed_mm_min",
            serde_json::json!(999.0),
        ),
        (
            "/integration/machine_output_enabled",
            serde_json::json!(true),
        ),
        ("/schema", serde_json::json!("future.schema")),
    ] {
        let mut forged = original.clone();
        *forged.pointer_mut(pointer).unwrap() = value;
        let bytes = serde_json::to_vec(&forged).unwrap();
        assert!(openwam::preview(&bytes, &catalog).is_err(), "{pointer}");
        assert!(openwam::simulate(&bytes, &catalog).is_err(), "{pointer}");
    }
    let mut unknown = original.clone();
    unknown["account_token"] = serde_json::json!("unused");
    assert!(openwam::reopen(&serde_json::to_vec(&unknown).unwrap(), &catalog).is_err());
    let mut oversized = original;
    oversized["source_text"] =
        serde_json::json!("x".repeat(reversal_sketch_intake::MAX_INPUT_BYTES + 1));
    assert!(openwam::reopen(&serde_json::to_vec(&oversized).unwrap(), &catalog).is_err());
}

#[test]
fn offline_preview_uses_verified_coordinates_and_never_embeds_source_markup() {
    let source = std::str::from_utf8(SVG).unwrap().replace(
        "id=\"line\"",
        "id=\"line\" data-note=\"&lt;script&gt;bad&lt;/script&gt;\"",
    );
    let html = openwam::preview(
        &project(source.as_bytes(), SourceFormat::Svg),
        &cutting::bundled().unwrap(),
    )
    .unwrap();
    let document = scraper::Html::parse_document(&html);
    let lines = document
        .select(&scraper::Selector::parse("svg line").unwrap())
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 2); // one rapid and one cut
    let cut = lines[1].value();
    let number = |key| cut.attr(key).unwrap().parse::<f64>().unwrap();
    assert!((number("x1") - 6.0).abs() < 1e-5);
    assert!((number("y1") - 8.0).abs() < 1e-5);
    assert!((number("x2") - 16.0).abs() < 1e-5);
    assert!((number("y2") - 8.0).abs() < 1e-5);
    assert_eq!(
        document
            .select(&scraper::Selector::parse("svg circle").unwrap())
            .count(),
        1
    );
    assert_eq!(
        document
            .select(&scraper::Selector::parse("script, link, img, iframe").unwrap())
            .count(),
        0
    );
    assert!(!html.contains("data-note") && !html.contains("bad"));
    assert!(html.contains("default-src 'none'") && html.contains("not a machine cut file"));
}
