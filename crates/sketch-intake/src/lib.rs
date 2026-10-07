//! Sketch inspection, agent workflows and unqualified waterjet research output.
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};
use usvg::tiny_skia_path::{PathSegment, Point, Transform};

pub mod bridge;
pub mod cam;
pub mod cutting;
pub mod dxf_intake;
mod json_identity;
pub mod openwam;
pub mod openwam_api;
pub mod parity;
pub mod simulation;
#[cfg(feature = "ui")]
pub mod ui;

pub const MAX_INPUT_BYTES: usize = 1024 * 1024;
const MAX_SEGMENTS: usize = 100_000;
const MM_PER_PX: f64 = 25.4 / 96.0;

#[derive(Debug, Serialize)]
pub struct Sketch {
    pub schema: &'static str,
    pub source_sha256: String,
    pub units: &'static str,
    pub coordinate_system: &'static str,
    pub viewport_mm: [f64; 2],
    pub machine_output_enabled: bool,
    pub contours: Vec<Contour>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Contour {
    pub source_id: String,
    pub closed: bool,
    pub segments: Vec<Segment>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Segment {
    Move {
        to: [f64; 2],
    },
    Line {
        to: [f64; 2],
    },
    Quadratic {
        control: [f64; 2],
        to: [f64; 2],
    },
    Cubic {
        controls: [[f64; 2]; 2],
        to: [f64; 2],
    },
    Close,
}

/// Import visible vector centrelines. SVG fill/stroke appearance is not a cut policy.
/// Arcs are normalized by usvg to cubic curves; curves are not flattened here.
pub fn inspect_svg(bytes: &[u8]) -> Result<Sketch> {
    ensure!(
        bytes.len() <= MAX_INPUT_BYTES,
        "SVG exceeds 1 MiB input limit"
    );
    let source = std::str::from_utf8(bytes).context("SVG must be UTF-8 XML")?;
    let doc = roxmltree::Document::parse(source).context("invalid SVG XML")?;
    let root = doc.root_element();
    ensure!(root.tag_name().name() == "svg", "expected SVG root");
    ensure!(
        root.tag_name().namespace() == Some("http://www.w3.org/2000/svg"),
        "SVG namespace required"
    );
    for key in ["width", "height"] {
        let value = root
            .attribute(key)
            .with_context(|| format!("explicit {key} required; scale must not be guessed"))?;
        let length: svgtypes::Length = value.parse().with_context(|| format!("invalid {key}"))?;
        ensure!(
            length.number.is_finite() && length.number > 0.0,
            "{key} must be positive and finite"
        );
        ensure!(
            matches!(
                length.unit,
                svgtypes::LengthUnit::None
                    | svgtypes::LengthUnit::Px
                    | svgtypes::LengthUnit::Mm
                    | svgtypes::LengthUnit::Cm
                    | svgtypes::LengthUnit::In
                    | svgtypes::LengthUnit::Pt
                    | svgtypes::LengthUnit::Pc
            ),
            "absolute {key} required; relative units are ambiguous"
        );
    }
    let mut input_segments = 0;
    for node in doc.descendants().filter(|n| n.is_element()) {
        ensure!(
            node.ancestors().count() <= 128,
            "SVG nesting exceeds 128 levels"
        );
        // Editor metadata is retained by the source digest, not interpreted as geometry.
        if node.ancestors().any(|n| {
            n.has_tag_name("metadata")
                || (n.tag_name().name() == "namedview"
                    && n.tag_name().namespace()
                        == Some("http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd"))
        }) {
            continue;
        }
        let tag = node.tag_name().name();
        ensure!(
            node.tag_name().namespace() == root.tag_name().namespace(),
            "unsupported geometry namespace"
        );
        ensure!(
            matches!(
                tag,
                "svg"
                    | "g"
                    | "defs"
                    | "use"
                    | "path"
                    | "rect"
                    | "circle"
                    | "ellipse"
                    | "line"
                    | "polyline"
                    | "polygon"
                    | "title"
                    | "desc"
                    | "style"
            ),
            "unsupported SVG element <{tag}>; convert text/images/effects to plain paths first"
        );
        ensure!(
            tag != "svg" || node == root,
            "nested SVG viewports are not supported yet"
        );
        for attr in node.attributes() {
            if attr.name() == "href" {
                ensure!(
                    attr.value().starts_with('#'),
                    "external SVG references are unsupported"
                );
            }
            ensure!(
                !matches!(
                    attr.name(),
                    "clip-path"
                        | "mask"
                        | "filter"
                        | "requiredFeatures"
                        | "requiredExtensions"
                        | "systemLanguage"
                ),
                "unsupported SVG attribute {}",
                attr.name()
            );
        }
        if let Some(data) = node.attribute("d").filter(|_| tag == "path") {
            for segment in svgtypes::PathParser::from(data) {
                let segment = segment.context("invalid SVG path data")?;
                ensure!(
                    finite_path_segment(segment),
                    "nonfinite SVG path coordinate"
                );
                input_segments += 1;
                ensure!(input_segments <= MAX_SEGMENTS, "SVG exceeds segment limit");
            }
        }
        if let Some(value) = node.attribute("transform") {
            for transform in svgtypes::TransformListParser::from(value) {
                transform.context("invalid SVG transform")?;
            }
        }
    }
    let tree = usvg::Tree::from_str(source, &usvg::Options::default())
        .context("SVG normalization failed")?;
    ensure!(
        tree.clip_paths().is_empty() && tree.masks().is_empty() && tree.filters().is_empty(),
        "clipping, masking and filters cannot be interpreted as cut geometry"
    );
    let mut sketch = Sketch {
        schema: "reversal.sketch-preview.v1",
        source_sha256: format!("{:x}", Sha256::digest(bytes)),
        units: "mm",
        coordinate_system: "svg_top_left_y_down",
        viewport_mm: [f64::from(tree.size().width()) * MM_PER_PX, f64::from(tree.size().height()) * MM_PER_PX],
        machine_output_enabled: false,
        contours: Vec::new(),
        diagnostics: vec!["Preview of visible vector centrelines only: fill/stroke widths, topology, self-intersections, duplicate edges and viewport clipping are not cut-validated.".into()],
    };
    collect(tree.root(), &mut sketch, &mut 0)?;
    ensure!(
        !sketch.contours.is_empty(),
        "SVG contains no supported visible vector geometry"
    );
    // Deterministic document-local identities for anonymous and compound paths.
    // Existing unique named paths retain their identity for project compatibility.
    let mut occurrences = std::collections::BTreeMap::new();
    for c in &sketch.contours {
        *occurrences.entry(c.source_id.clone()).or_insert(0usize) += 1;
    }
    let mut sequence = std::collections::BTreeMap::new();
    let mut used = std::collections::BTreeSet::new();
    for (index, c) in sketch.contours.iter_mut().enumerate() {
        if c.source_id.is_empty() {
            c.source_id = format!("svg-contour-{index}");
        } else if occurrences[&c.source_id] > 1 {
            let order = sequence.entry(c.source_id.clone()).or_insert(0usize);
            c.source_id = format!("{}/subpath-{}", c.source_id, *order);
            *order += 1;
        }
        ensure!(
            used.insert(c.source_id.clone()),
            "generated SVG identity collides with source ID; rename the conflicting source ID"
        );
    }
    let open = sketch.contours.iter().filter(|c| !c.closed).count();
    if open != 0 {
        sketch.diagnostics.push(format!(
            "{open} open subpaths; no automatic closing or endpoint snapping performed."
        ));
    }
    Ok(sketch)
}

fn finite_path_segment(segment: svgtypes::PathSegment) -> bool {
    use svgtypes::PathSegment::*;
    match segment {
        MoveTo { x, y, .. } | LineTo { x, y, .. } | SmoothQuadratic { x, y, .. } => {
            [x, y].into_iter().all(f64::is_finite)
        }
        HorizontalLineTo { x, .. } => x.is_finite(),
        VerticalLineTo { y, .. } => y.is_finite(),
        CurveTo {
            x1,
            y1,
            x2,
            y2,
            x,
            y,
            ..
        } => [x1, y1, x2, y2, x, y].into_iter().all(f64::is_finite),
        SmoothCurveTo { x2, y2, x, y, .. } => [x2, y2, x, y].into_iter().all(f64::is_finite),
        Quadratic { x1, y1, x, y, .. } => [x1, y1, x, y].into_iter().all(f64::is_finite),
        EllipticalArc {
            rx,
            ry,
            x_axis_rotation,
            x,
            y,
            ..
        } => [rx, ry, x_axis_rotation, x, y]
            .into_iter()
            .all(f64::is_finite),
        ClosePath { .. } => true,
    }
}

fn point(mut p: Point, transform: Transform) -> Result<[f64; 2]> {
    transform.map_point(&mut p);
    ensure!(
        p.x.is_finite() && p.y.is_finite(),
        "transformed coordinates exceed finite range"
    );
    Ok([f64::from(p.x) * MM_PER_PX, f64::from(p.y) * MM_PER_PX])
}

fn collect(group: &usvg::Group, sketch: &mut Sketch, count: &mut usize) -> Result<()> {
    for node in group.children() {
        match node {
            usvg::Node::Group(g) => collect(g, sketch, count)?,
            usvg::Node::Path(path) if path.is_visible() => {
                let transform = path.abs_transform();
                let mut current: Option<Contour> = None;
                for segment in path.data().segments() {
                    *count += 1;
                    ensure!(
                        *count <= MAX_SEGMENTS,
                        "expanded geometry exceeds segment limit"
                    );
                    if matches!(segment, PathSegment::MoveTo(_)) {
                        if let Some(c) = current.take() {
                            sketch.contours.push(c);
                        }
                        current = Some(Contour {
                            source_id: path.id().to_owned(),
                            closed: false,
                            segments: Vec::new(),
                        });
                    }
                    let contour = current.as_mut().context("normalized path missing move")?;
                    // Another drawing command after Z makes the ending open again.
                    if !matches!(segment, PathSegment::Close) {
                        contour.closed = false;
                    }
                    contour.segments.push(match segment {
                        PathSegment::MoveTo(p) => Segment::Move {
                            to: point(p, transform)?,
                        },
                        PathSegment::LineTo(p) => Segment::Line {
                            to: point(p, transform)?,
                        },
                        PathSegment::QuadTo(c, p) => Segment::Quadratic {
                            control: point(c, transform)?,
                            to: point(p, transform)?,
                        },
                        PathSegment::CubicTo(c1, c2, p) => Segment::Cubic {
                            controls: [point(c1, transform)?, point(c2, transform)?],
                            to: point(p, transform)?,
                        },
                        PathSegment::Close => {
                            contour.closed = true;
                            Segment::Close
                        }
                    });
                }
                if let Some(c) = current {
                    sketch.contours.push(c);
                }
            }
            usvg::Node::Path(_) => {}
            _ => bail!("normalized SVG contains non-vector geometry"),
        }
    }
    Ok(())
}
