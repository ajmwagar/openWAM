//! `fileBB` as WAM 2.4 computes it from an embedded SVG.
//!
//! Observed rule (docs/wzr-format.md §fileBB): only `<path>` elements count
//! (Foam Insert's `<rect>`/`<circle>` are ignored), stroke width is ignored,
//! curve extrema are exact, user units are mapped to CSS px through the root
//! `width`/`viewBox` ratio (px when `width` is absent), and inches = px / 96.

use kurbo::{Affine, BezPath, Rect, Shape};

#[derive(Debug, thiserror::Error)]
pub enum BBoxError {
    #[error("SVG XML: {0}")]
    Xml(#[from] roxmltree::Error),
    #[error("root element is not <svg>")]
    NotSvg,
    #[error("unsupported length {0:?} on the root <svg>")]
    Length(String),
    #[error("bad path data in <path>: {0}")]
    PathData(String),
    #[error("bad transform {0:?}")]
    Transform(String),
    #[error("SVG has no <path> geometry WAM would import")]
    Empty,
}

/// Inch box `(min_x, min_y, max_x, max_y)` of all `<path>` geometry.
pub fn wam_file_bb(svg: &str) -> Result<Rect, BBoxError> {
    let doc = roxmltree::Document::parse(svg)?;
    let root = doc.root_element();
    if root.tag_name().name() != "svg" {
        return Err(BBoxError::NotSvg);
    }
    let px_per_user = root_scale(&root)?;
    let mut acc: Option<Rect> = None;
    for node in root
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "path")
    {
        let mut ctm = Affine::IDENTITY;
        let mut hidden = false;
        for anc in node.ancestors().filter(|n| n.is_element()) {
            if matches!(
                anc.tag_name().name(),
                "defs" | "clipPath" | "mask" | "symbol" | "pattern" | "marker"
            ) {
                hidden = true;
            }
            if let Some(t) = anc.attribute("transform") {
                ctm = parse_transform(t)? * ctm;
            }
        }
        if hidden {
            continue;
        }
        let Some(d) = node.attribute("d") else {
            continue;
        };
        let path = BezPath::from_svg(d).map_err(|e| BBoxError::PathData(e.to_string()))?;
        if path.elements().is_empty() {
            continue;
        }
        let b = (ctm * path).bounding_box();
        acc = Some(acc.map_or(b, |a| a.union(b)));
    }
    let b = acc.ok_or(BBoxError::Empty)?;
    let k = px_per_user / 96.0;
    Ok(Rect::new(b.x0 * k, b.y0 * k, b.x1 * k, b.y1 * k))
}

/// Accumulated `transform` of `node` and its ancestors, and whether it sits in a
/// non-rendered container (`defs`, `clipPath`, ...).
pub(crate) fn node_ctm(node: &roxmltree::Node<'_, '_>) -> Result<(Affine, bool), BBoxError> {
    let mut ctm = Affine::IDENTITY;
    let mut hidden = false;
    for anc in node.ancestors().filter(|n| n.is_element()) {
        if matches!(
            anc.tag_name().name(),
            "defs" | "clipPath" | "mask" | "symbol" | "pattern" | "marker"
        ) {
            hidden = true;
        }
        if let Some(t) = anc.attribute("transform") {
            ctm = parse_transform(t)? * ctm;
        }
    }
    Ok((ctm, hidden))
}

fn parse_transform(t: &str) -> Result<Affine, BBoxError> {
    let tr = svgtypes::Transform::from_str(t).map_err(|_| BBoxError::Transform(t.into()))?;
    Ok(Affine::new([tr.a, tr.b, tr.c, tr.d, tr.e, tr.f]))
}

fn length_px(s: &str) -> Result<f64, BBoxError> {
    let l = svgtypes::Length::from_str(s).map_err(|_| BBoxError::Length(s.into()))?;
    use svgtypes::LengthUnit as U;
    let k = match l.unit {
        U::None | U::Px => 1.0,
        U::Mm => 96.0 / 25.4,
        U::Cm => 96.0 / 2.54,
        U::In => 96.0,
        U::Pt => 96.0 / 72.0,
        U::Pc => 16.0,
        _ => return Err(BBoxError::Length(s.into())),
    };
    Ok(l.number * k)
}

/// CSS px per SVG user unit (x axis; WAM samples are all uniform).
fn root_scale(root: &roxmltree::Node<'_, '_>) -> Result<f64, BBoxError> {
    let view_w = root
        .attribute("viewBox")
        .and_then(|v| svgtypes::ViewBox::from_str(v).ok())
        .map(|vb| vb.w);
    match (root.attribute("width"), view_w) {
        (Some(w), Some(vw)) if vw > 0.0 => Ok(length_px(w)? / vw),
        _ => Ok(1.0),
    }
}

use std::str::FromStr;
