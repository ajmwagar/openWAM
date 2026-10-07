use reversal_sketch_intake::{Segment, inspect_svg};

fn svg(body: &str) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="50mm" viewBox="0 0 100 50">{body}</svg>"#
    )
}
fn near(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 0.00002,
        "{actual} != {expected}"
    );
}

#[test]
fn physical_units_viewbox_and_nested_transforms_are_applied_once() {
    let s = inspect_svg(svg(r#"<g transform="translate(10 5)"><g transform="scale(2)"><path id="part" d="M1 2 L3 2 L3 4 Z"/></g></g>"#).as_bytes()).unwrap();
    near(s.viewport_mm[0], 100.0);
    near(s.viewport_mm[1], 50.0);
    assert!(s.contours[0].closed);
    assert_eq!(s.contours[0].source_id, "part");
    if let Segment::Move { to } = s.contours[0].segments[0] {
        near(to[0], 12.0);
        near(to[1], 9.0);
    } else {
        panic!();
    }
    assert!(!s.machine_output_enabled);
}

#[test]
fn subpaths_keep_explicit_closure_and_curves() {
    let s = inspect_svg(svg(r#"<path d="M0 0 C1 2 3 4 5 6 Z M10 10 Q11 12 13 14"/>"#).as_bytes())
        .unwrap();
    assert_eq!(s.contours.len(), 2);
    assert!(s.contours[0].closed);
    assert!(!s.contours[1].closed);
    assert!(matches!(s.contours[0].segments[1], Segment::Cubic { .. }));
    assert!(matches!(
        s.contours[1].segments[1],
        Segment::Quadratic { .. }
    ));
    assert!(s.diagnostics.iter().any(|d| d.contains("1 open subpaths")));
}

#[test]
fn shapes_and_local_reuse_work() {
    let source = svg(
        r##"<defs><rect id="r" width="10" height="5"/></defs><use href="#r" x="20"/><circle cx="40" cy="20" r="5"/>"##,
    );
    let s = inspect_svg(source.as_bytes()).unwrap();
    assert_eq!(s.contours.len(), 2);
    assert!(s.contours.iter().all(|c| c.closed));
    if let Segment::Move { to } = s.contours[0].segments[0] {
        near(to[0], 20.0);
    } else {
        panic!();
    }
}

#[test]
fn drawing_after_close_is_not_reported_as_a_closed_ending() {
    let s = inspect_svg(svg(r#"<path d="M0 0 L5 0 L5 5 Z L10 10"/>"#).as_bytes()).unwrap();
    assert!(!s.contours.last().unwrap().closed);
}

#[test]
fn ambiguous_dimensions_and_unsupported_content_fail_loudly() {
    for source in [
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="5" height="5"/></svg>"#.to_owned(),
        svg(r#"<text>Part</text>"#),
        svg(r#"<image href="file:///private/image.svg"/>"#),
        svg(r#"<use href="https://example.com/part.svg#p"/>"#),
        svg(r#"<path d="M0 0 L1 nope"/>"#),
        svg(r#"<path d="M0 0 L1e400 1"/>"#),
        svg(r#"<path transform="broken(2)" d="M0 0L2 2"/>"#),
        svg(r#"<clipPath id="c"><rect width="1" height="1"/></clipPath>"#),
        svg(r#"<script>alert(1)</script>"#),
        svg(""),
    ] { assert!(inspect_svg(source.as_bytes()).is_err(), "accepted {source}"); }
}

#[test]
fn inch_scale_and_aspect_ratio_match_svg_semantics() {
    let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="1in" height="1in" viewBox="0 0 10 20"><path d="M0 0 L10 20"/></svg>"#;
    let s = inspect_svg(source.as_bytes()).unwrap();
    near(s.viewport_mm[0], 25.4);
    if let Segment::Move { to } = s.contours[0].segments[0] {
        near(to[0], 6.35);
        near(to[1], 0.0);
    } else {
        panic!();
    }
}

#[test]
fn identical_inputs_produce_identical_reports_and_oversize_is_refused() {
    let source = svg(r#"<path d="M0 0L10 10"/>"#);
    let a = inspect_svg(source.as_bytes()).unwrap();
    let b = inspect_svg(source.as_bytes()).unwrap();
    assert_eq!(
        serde_json::to_vec(&a).unwrap(),
        serde_json::to_vec(&b).unwrap()
    );
    assert!(inspect_svg(&vec![b' '; 1024 * 1024 + 1]).is_err());
}

#[test]
fn anonymous_and_compound_paths_have_unique_deterministic_identities() {
    let source=br#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100"><sodipodi:namedview xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd"/><path d="M10 10H90V90H10Z"/><path id="holes" d="M20 20H30V30H20Z M40 40H50V50H40Z"/></svg>"#;
    let a = reversal_sketch_intake::inspect_svg(source).unwrap();
    let b = reversal_sketch_intake::inspect_svg(source).unwrap();
    let ids: Vec<_> = a.contours.iter().map(|c| c.source_id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["svg-contour-0", "holes/subpath-0", "holes/subpath-1"]
    );
    assert_eq!(
        serde_json::to_value(a).unwrap(),
        serde_json::to_value(b).unwrap()
    );
}

#[test]
fn generated_contour_id_collisions_are_refused() {
    for content in [
        r#"<path d="M10 10H20V20H10Z"/><path id="svg-contour-0" d="M30 30H40V40H30Z"/>"#,
        r#"<path id="holes" d="M10 10H20V20H10Z M30 30H40V40H30Z"/><path id="holes/subpath-0" d="M50 50H60V60H50Z"/>"#,
    ] {
        assert!(inspect_svg(svg(content).as_bytes()).is_err());
    }
}
