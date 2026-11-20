//! Explicit adapter to the Transmog sketch contract and Swarf waterjet owner.
use crate::{
    Segment, Sketch,
    cutting::{self, Catalog, Model, Units, UserMaterial},
};
use anyhow::{Context, Result, ensure};
use kurbo::{CubicBez, ParamCurve, ParamCurveArclen, Point, QuadBez};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use transmog_core::{
    geometry::Point2,
    ir::{Feature, PlaneRef, SketchSegment},
};

const MAX_LINES: usize = 100_000;

/// Expanded copies are bounded so a layout cannot exhaust the byte, point,
/// line or operation budgets before the owners see it.
pub const MAX_INSTANCES: usize = 64;

/// Explicit multi-copy layout. Omitting it keeps the single-instance contract.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub instances: Vec<LayoutInstance>,
}

/// One placed copy of the retained source, never a copy of the source bytes.
/// `q = R(rotation_deg) * scale * (p - c) + c + placement`, where `p` and `c`
/// are the contour point and source bounding-box centre in the local CAD frame
/// (Y negative down) and rotation is counterclockwise about +Z.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LayoutInstance {
    pub rotation_deg: f64,
    pub scale: f64,
    pub placement_xy_down_mm: [f64; 2],
}

#[derive(Debug, Serialize)]
pub struct ContourIdentity {
    pub source_id: String,
    pub closed: bool,
}

#[derive(Debug, Serialize)]
pub struct Geometry {
    pub schema: &'static str,
    pub source_sha256: String,
    pub coordinate_system: &'static str,
    pub placement_xy_down_mm: [f64; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<Layout>,
    pub requested_curve_tolerance_mm: f64,
    pub max_bezier_chord_bound_mm: f64,
    pub features: Vec<Feature>,
    pub contour_identity: Vec<ContourIdentity>,
    /// Present only for a multi-source bundle; absent keeps single-source JSON unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<SourceRecord>>,
    pub diagnostics: Vec<String>,
}

/// Maximum sources in one bundle.
pub const MAX_SOURCES: usize = 16;
/// Upper bound for the serialized bundle text (each source is still capped at 1 MiB).
pub const MAX_BUNDLE_BYTES: usize = 16 * 1024 * 1024;
pub const BUNDLE_SCHEMA: &str = "reversal.openwam-sources.v1";

/// One imported design. `placement_xy_down_mm` / `layout` follow the single-source
/// contract per source; `contours` keeps a subset (a split cut group).
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourceSpec {
    /// 1..=32 chars of `[A-Za-z0-9_-]`, unique in the bundle.
    pub id: String,
    /// File name for display and export; no path separators.
    pub name: String,
    pub format: SourceFormat,
    pub text: String,
    pub placement_xy_down_mm: [f64; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<Layout>,
    /// Keep only these contours, named by the source's own contour ids.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contours: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourceBundle {
    pub schema: String,
    pub sources: Vec<SourceSpec>,
}

impl SourceBundle {
    pub fn new(sources: Vec<SourceSpec>) -> Self {
        Self {
            schema: BUNDLE_SCHEMA.into(),
            sources,
        }
    }
    pub fn to_text(&self) -> Result<String> {
        let text = serde_json::to_string(self)?;
        ensure!(
            text.len() <= MAX_BUNDLE_BYTES,
            "source bundle exceeds {MAX_BUNDLE_BYTES} bytes"
        );
        Ok(text)
    }
}

/// Where one source's copies landed (bed frame, mm, Y down) and which contours
/// belong to it. Contour ids are the combined `<source id>:<contour id>` form;
/// a layout copy appends `#<instance>`.
#[derive(Debug, Serialize)]
pub struct SourceRecord {
    pub id: String,
    pub name: String,
    pub format: SourceFormat,
    pub source_sha256: String,
    /// The source's own viewport (mm), before the bundle takes the maximum.
    pub viewport_mm: [f64; 2],
    /// True when contour ids carry a `#<copy>` suffix because a layout was applied.
    pub layout_applied: bool,
    pub contour_ids: Vec<String>,
    /// Gap-rule changes made while importing this source (merged, closed, dropped).
    pub normalized: Vec<String>,
    /// Paths the gap rules could not close without a zero-area loop (blocking).
    pub degenerate: Vec<String>,
    pub instances: Vec<InstanceRecord>,
}

#[derive(Debug, Serialize)]
pub struct InstanceRecord {
    pub rotation_deg: f64,
    pub scale: f64,
    pub placement_xy_down_mm: [f64; 2],
    pub bbox_min_xy_down_mm: [f64; 2],
    pub bbox_max_xy_down_mm: [f64; 2],
}

struct Flatten {
    tolerance: f64,
    max_bound: f64,
    count: usize,
}
impl Flatten {
    fn line(&mut self, a: Point, b: Point, out: &mut Vec<SketchSegment>) -> Result<()> {
        if a == b {
            return Ok(());
        }
        ensure!(
            a.x.is_finite() && a.y.is_finite() && b.x.is_finite() && b.y.is_finite(),
            "nonfinite geometry"
        );
        self.count += 1;
        ensure!(
            self.count <= MAX_LINES,
            "flattened geometry exceeds 100000 lines"
        );
        out.push(SketchSegment::Line {
            start: Point2::new(a.x, a.y),
            end: Point2::new(b.x, b.y),
        });
        Ok(())
    }
    fn cubic(&mut self, curve: CubicBez, depth: usize, out: &mut Vec<SketchSegment>) -> Result<()> {
        // Compare Bezier controls against the degree-elevated chord at equal t.
        // Convexity bounds |B(t)-L(t)| for every t, not just sampled midpoints.
        let delta = curve.p3 - curve.p0;
        let bound = curve
            .p1
            .distance(curve.p0 + delta / 3.0)
            .max(curve.p2.distance(curve.p0 + delta * (2.0 / 3.0)));
        ensure!(bound.is_finite(), "nonfinite curve bound");
        if bound <= self.tolerance {
            self.max_bound = self.max_bound.max(bound);
            self.line(curve.p0, curve.p3, out)
        } else {
            ensure!(depth < 32, "curve subdivision depth exhausted");
            self.cubic(curve.subsegment(0.0..0.5), depth + 1, out)?;
            self.cubic(curve.subsegment(0.5..1.0), depth + 1, out)
        }
    }
}

/// Flatten every retained contour into line segments, mapping source coordinates
/// through `map` first. Nothing is snapped, joined or re-ordered here.
fn flatten_contours(
    sketch: &Sketch,
    flatten: &mut Flatten,
    map: impl Fn([f64; 2]) -> Point,
) -> Result<(Vec<Vec<SketchSegment>>, Vec<ContourIdentity>)> {
    let mut profiles = Vec::new();
    let mut identity = Vec::new();
    for c in &sketch.contours {
        let mut at = None;
        let mut start = None;
        let mut profile = Vec::new();
        for segment in &c.segments {
            match segment {
                Segment::Move { to } => {
                    ensure!(at.is_none(), "multiple moves in contour");
                    at = Some(map(*to));
                    start = at;
                }
                Segment::Line { to } => {
                    let next = map(*to);
                    flatten.line(at.context("missing contour start")?, next, &mut profile)?;
                    at = Some(next);
                }
                Segment::Quadratic { control, to } => {
                    let a = at.context("missing contour start")?;
                    let b = map(*to);
                    let q = map(*control);
                    flatten.cubic(
                        CubicBez::new(a, a + (q - a) * (2.0 / 3.0), b + (q - b) * (2.0 / 3.0), b),
                        0,
                        &mut profile,
                    )?;
                    at = Some(b);
                }
                Segment::Cubic { controls, to } => {
                    let next = map(*to);
                    flatten.cubic(
                        CubicBez::new(
                            at.context("missing contour start")?,
                            map(controls[0]),
                            map(controls[1]),
                            next,
                        ),
                        0,
                        &mut profile,
                    )?;
                    at = Some(next);
                }
                Segment::Close => {
                    let next = start.context("missing close start")?;
                    flatten.line(at.context("missing close start")?, next, &mut profile)?;
                    at = Some(next);
                }
            }
        }
        ensure!(!profile.is_empty(), "degenerate contour");
        ensure!(!c.closed || at == start, "closed contour has endpoint gap");
        profiles.push(profile);
        identity.push(ContourIdentity {
            source_id: c.source_id.clone(),
            closed: c.closed,
        });
    }
    ensure!(!profiles.is_empty(), "empty sketch");
    Ok((profiles, identity))
}

/// Reject unusable layouts instead of repairing them. Mirroring, non-finite
/// transforms and an ambiguous stock placement are not silently reinterpreted.
fn validate_layout(layout: &Layout, placement: [f64; 2]) -> Result<()> {
    ensure!(
        !layout.instances.is_empty(),
        "layout requires at least one instance"
    );
    ensure!(
        layout.instances.len() <= MAX_INSTANCES,
        "layout exceeds {MAX_INSTANCES} instances"
    );
    ensure!(
        placement == [0.0, 0.0],
        "layout requires placement_xy_down_mm [0,0]; per-instance placement is authoritative"
    );
    for (index, instance) in layout.instances.iter().enumerate() {
        ensure!(
            instance.rotation_deg.is_finite(),
            "instance {index}: rotation must be finite degrees"
        );
        ensure!(
            instance.scale.is_finite() && instance.scale > 0.0,
            "instance {index}: scale must be finite and greater than zero; mirroring is unsupported"
        );
        ensure!(
            instance.placement_xy_down_mm.iter().all(|v| v.is_finite()),
            "instance {index}: placement must be finite millimetres"
        );
    }
    Ok(())
}

/// Centre of the flattened bounding box in the local CAD frame (Y negative down).
fn bounding_box_centre(profiles: &[Vec<SketchSegment>]) -> Result<[f64; 2]> {
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for segment in profiles.iter().flatten() {
        let SketchSegment::Line { start, end } = segment else {
            continue;
        };
        for p in [start, end] {
            min = [min[0].min(p.x), min[1].min(p.y)];
            max = [max[0].max(p.x), max[1].max(p.y)];
        }
    }
    ensure!(
        min.iter().chain(&max).all(|v| v.is_finite()),
        "layout source has no finite bounds"
    );
    Ok([(min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0])
}

/// One placed copy: uniform scale and rotation about `centre`, then placement.
fn place_profile(
    profile: &[SketchSegment],
    instance: &LayoutInstance,
    centre: [f64; 2],
) -> Result<Vec<SketchSegment>> {
    let (sin, cos) = instance.rotation_deg.to_radians().sin_cos();
    let scale = instance.scale;
    let [placement_x, placement_y] = instance.placement_xy_down_mm;
    let map = |p: Point2| -> Result<Point2> {
        let dx = p.x - centre[0];
        let dy = p.y - centre[1];
        let x = centre[0] + scale * (cos * dx - sin * dy) + placement_x;
        let y = centre[1] + scale * (sin * dx + cos * dy) - placement_y;
        ensure!(x.is_finite() && y.is_finite(), "nonfinite placed geometry");
        Ok(Point2::new(x, y))
    };
    profile
        .iter()
        .map(|segment| {
            let SketchSegment::Line { start, end } = segment else {
                anyhow::bail!("adapter emits only line profiles")
            };
            Ok(SketchSegment::Line {
                start: map(*start)?,
                end: map(*end)?,
            })
        })
        .collect()
}

/// Reflect Y to CAD XY and apply only the explicitly supplied stock placement.
/// With a layout, one placed copy per instance replaces the single placement.
pub fn to_transmog(
    sketch: &Sketch,
    tolerance: f64,
    placement: [f64; 2],
    layout: Option<&Layout>,
) -> Result<Geometry> {
    ensure!(
        tolerance.is_finite() && (0.000001..=1.0).contains(&tolerance),
        "curve tolerance must be 0.000001..1 mm"
    );
    ensure!(placement.iter().all(|v| v.is_finite()), "invalid placement");
    ensure!(
        sketch.units == "mm"
            && sketch.coordinate_system == "svg_top_left_y_down"
            && !sketch.machine_output_enabled,
        "unsupported sketch contract"
    );
    let mut flatten = Flatten {
        tolerance,
        max_bound: 0.0,
        count: 0,
    };
    let (features, identity) = match layout {
        None => flatten_contours(sketch, &mut flatten, |p: [f64; 2]| {
            Point::new(p[0] + placement[0], -p[1] - placement[1])
        })?,
        Some(layout) => {
            validate_layout(layout, placement)?;
            // The expansion is the whole retained source repeated per instance,
            // so the flattened-line and operation budgets apply after expansion.
            ensure!(
                sketch.contours.len().saturating_mul(layout.instances.len()) <= MAX_LINES,
                "layout expansion exceeds {MAX_LINES} flattened lines"
            );
            let (local, source) =
                flatten_contours(sketch, &mut flatten, |p: [f64; 2]| Point::new(p[0], -p[1]))?;
            let centre = bounding_box_centre(&local)?;
            let mut features = Vec::with_capacity(local.len() * layout.instances.len());
            let mut identity = Vec::with_capacity(source.len() * layout.instances.len());
            for (index, instance) in layout.instances.iter().enumerate() {
                for (profile, source) in local.iter().zip(&source) {
                    features.push(place_profile(profile, instance, centre)?);
                    identity.push(ContourIdentity {
                        source_id: format!("{}#{index}", source.source_id),
                        closed: source.closed,
                    });
                }
            }
            (features, identity)
        }
    };
    let mut diagnostics = vec![
        "Bezier bound is relative to normalized SVG curves; source SVG arc normalization and float32 transforms are additional upstream errors.".to_string(),
        "Each contour remains separate. No endpoint snapping, hole classification, topology certification or kerf offset is performed.".to_string(),
    ];
    if layout.is_some() {
        diagnostics.push("Layout copies are expanded geometry from the retained source: the reported Bezier chord bound is the unplaced flattening bound and per-instance scale multiplies it.".to_string());
    }
    Ok(Geometry {
        schema: "reversal.transmog-sketch-bridge.v1",
        source_sha256: sketch.source_sha256.clone(),
        coordinate_system: "xy_mm_top_left_y_negative_down",
        placement_xy_down_mm: placement,
        layout: layout.cloned(),
        requested_curve_tolerance_mm: tolerance,
        max_bezier_chord_bound_mm: flatten.max_bound,
        features: features
            .into_iter()
            .map(|profile| Feature::Sketch {
                plane: PlaneRef::Xy,
                profile,
            })
            .collect(),
        contour_identity: identity,
        sources: None,
        diagnostics,
    })
}

fn valid_id(id: &str) -> bool {
    (1..=32).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= 128
        && !name.contains(['/', '\\'])
        && !name.contains("..")
        && !name.chars().any(char::is_control)
}

fn bbox_down(features: &[Feature]) -> Result<([f64; 2], [f64; 2])> {
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for feature in features {
        let Feature::Sketch { profile, .. } = feature else {
            continue;
        };
        for segment in profile {
            let SketchSegment::Line { start, end } = segment else {
                anyhow::bail!("adapter emits only line profiles")
            };
            for p in [start, end] {
                min = [min[0].min(p.x), min[1].min(-p.y)];
                max = [max[0].max(p.x), max[1].max(-p.y)];
            }
        }
    }
    ensure!(
        min.iter().chain(&max).all(|v| v.is_finite()),
        "source has no finite bounds"
    );
    Ok((min, max))
}

/// WAM import gap rules (mm): open fragments whose ends are this close are merged.
pub const GAP_MERGE_MM: f64 = 0.025;
/// A path whose own ends are closer than this is closed.
pub const GAP_CLOSE_MM: f64 = 0.051;
/// Unconnected segments shorter than this are dropped.
pub const GAP_DROP_MM: f64 = 0.0025;

fn seg_end(s: &Segment) -> Option<[f64; 2]> {
    match s {
        Segment::Move { to }
        | Segment::Line { to }
        | Segment::Quadratic { to, .. }
        | Segment::Cubic { to, .. } => Some(*to),
        Segment::Close => None,
    }
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn open_ends(c: &crate::Contour) -> Option<([f64; 2], [f64; 2])> {
    if c.closed || c.segments.iter().any(|s| matches!(s, Segment::Close)) {
        return None;
    }
    let start = c.segments.first().and_then(seg_end)?;
    let end = c.segments.last().and_then(seg_end)?;
    Some((start, end))
}

/// True length of the path: curves are measured along the curve, not by their chord.
fn open_length(c: &crate::Contour) -> f64 {
    let mut at: Option<[f64; 2]> = None;
    let mut len = 0.0;
    for s in &c.segments {
        let Some(to) = seg_end(s) else { continue };
        if let Some(a) = at {
            let p = |q: [f64; 2]| Point::new(q[0], q[1]);
            len += match s {
                Segment::Quadratic { control, .. } => {
                    QuadBez::new(p(a), p(*control), p(to)).arclen(1e-6)
                }
                Segment::Cubic { controls, .. } => {
                    CubicBez::new(p(a), p(controls[0]), p(controls[1]), p(to)).arclen(1e-6)
                }
                _ => dist(a, to),
            };
        }
        at = Some(to);
    }
    len
}

/// Polygon of the path sampled along its curves, for an area test.
fn sampled(c: &crate::Contour) -> Vec<[f64; 2]> {
    let mut pts = Vec::new();
    let mut at: Option<[f64; 2]> = None;
    for s in &c.segments {
        let Some(to) = seg_end(s) else { continue };
        if let Some(a) = at {
            let p = |q: [f64; 2]| Point::new(q[0], q[1]);
            for k in 1..=16 {
                let t = f64::from(k) / 16.0;
                let q = match s {
                    Segment::Quadratic { control, .. } => {
                        QuadBez::new(p(a), p(*control), p(to)).eval(t)
                    }
                    Segment::Cubic { controls, .. } => {
                        CubicBez::new(p(a), p(controls[0]), p(controls[1]), p(to)).eval(t)
                    }
                    _ => Point::new(a[0] + (to[0] - a[0]) * t, a[1] + (to[1] - a[1]) * t),
                };
                pts.push([q.x, q.y]);
            }
        } else {
            pts.push(to);
        }
        at = Some(to);
    }
    pts
}

fn area(pts: &[[f64; 2]]) -> f64 {
    let n = pts.len();
    (0..n)
        .map(|i| pts[i][0] * pts[(i + 1) % n][1] - pts[(i + 1) % n][0] * pts[i][1])
        .sum::<f64>()
        / 2.0
}

/// Smallest area (mm^2) a closed contour must enclose.
const MIN_AREA_MM2: f64 = 1e-6;

fn reversed(c: &crate::Contour) -> crate::Contour {
    let mut points = Vec::new();
    for s in &c.segments {
        if let Some(p) = seg_end(s) {
            points.push(p);
        }
    }
    let n = points.len();
    let mut segments = vec![Segment::Move { to: points[n - 1] }];
    for i in (1..n).rev() {
        let to = points[i - 1];
        segments.push(match &c.segments[i] {
            Segment::Quadratic { control, .. } => Segment::Quadratic {
                control: *control,
                to,
            },
            Segment::Cubic { controls, .. } => Segment::Cubic {
                controls: [controls[1], controls[0]],
                to,
            },
            _ => Segment::Line { to },
        });
    }
    crate::Contour {
        source_id: c.source_id.clone(),
        closed: false,
        segments,
    }
}

/// Apply WAM's import gap rules to a sketch: drop unconnected pieces shorter
/// than 0.0025 mm, merge open fragments whose ends are within 0.025 mm, and
/// close paths whose own ends are within 0.051 mm. Returns one line per change.
pub fn apply_gap_rules(sketch: &mut Sketch) -> GapReport {
    let mut log = Vec::new();
    let mut degenerate = Vec::new();
    sketch.contours.retain(|c| {
        let tiny = open_ends(c).is_some() && open_length(c) < GAP_DROP_MM;
        if tiny {
            log.push(format!(
                "dropped {} (shorter than {GAP_DROP_MM} mm)",
                c.source_id
            ));
        }
        !tiny
    });
    loop {
        let mut best: Option<(usize, usize, bool, bool, f64)> = None;
        for i in 0..sketch.contours.len() {
            for j in 0..sketch.contours.len() {
                if i == j {
                    continue;
                }
                let (Some((_, ae)), Some((bs, be))) = (
                    open_ends(&sketch.contours[i]),
                    open_ends(&sketch.contours[j]),
                ) else {
                    continue;
                };
                // Join end of i to start of j, or to end of j (reversing j).
                for (rev_j, p) in [(false, bs), (true, be)] {
                    let d = dist(ae, p);
                    if d <= GAP_MERGE_MM && best.is_none_or(|b| d < b.4) {
                        best = Some((i, j, false, rev_j, d));
                    }
                }
            }
        }
        let Some((i, j, _, rev_j, d)) = best else {
            break;
        };
        let removed = sketch.contours.remove(j);
        let ai = if j < i { i - 1 } else { i };
        let b = if rev_j { reversed(&removed) } else { removed };
        log.push(format!(
            "merged {} into {} (gap {d:.4} mm)",
            b.source_id, sketch.contours[ai].source_id
        ));
        sketch.contours[ai]
            .segments
            .extend(b.segments.into_iter().skip(1));
    }
    for c in &mut sketch.contours {
        if let Some((start, end)) = open_ends(c) {
            let d = dist(start, end);
            // Closing wins over dropping: only a path longer than the drop limit is closed.
            if d < GAP_CLOSE_MM && open_length(c) >= GAP_DROP_MM {
                if area(&sampled(c)).abs() < MIN_AREA_MM2 {
                    degenerate.push(format!(
                        "{}: closing it would give a zero-area loop (overlapping or duplicate lines); remove the duplicate in the drawing",
                        c.source_id
                    ));
                    continue;
                }
                c.segments.push(Segment::Close);
                c.closed = true;
                log.push(format!("closed {} (gap {d:.4} mm)", c.source_id));
            }
        }
    }
    GapReport { log, degenerate }
}

/// What the gap rules changed, and what they could not resolve.
pub struct GapReport {
    pub log: Vec<String>,
    /// Contours the rules would have closed into a zero-area loop; left open, blocking.
    pub degenerate: Vec<String>,
}

/// Combine several sources into one sketch/geometry pair. Each source keeps its
/// own placement or layout; contour identities are prefixed with the source id
/// so the CAM intent can address every contour of every copy.
fn bundle_geometry(bytes: &[u8], tolerance: f64) -> Result<(Sketch, Geometry)> {
    use sha2::{Digest, Sha256};
    ensure!(
        bytes.len() <= MAX_BUNDLE_BYTES,
        "source bundle exceeds {MAX_BUNDLE_BYTES} bytes"
    );
    let bundle: SourceBundle =
        serde_json::from_slice(bytes).context("invalid source bundle JSON")?;
    ensure!(
        bundle.schema == BUNDLE_SCHEMA,
        "unsupported source bundle schema"
    );
    ensure!(
        (1..=MAX_SOURCES).contains(&bundle.sources.len()),
        "a bundle holds 1..={MAX_SOURCES} sources"
    );
    let mut ids = std::collections::BTreeSet::new();
    let mut features = Vec::new();
    let mut identity = Vec::new();
    let mut records = Vec::new();
    let mut contours = Vec::new();
    let mut diagnostics = Vec::new();
    let mut viewport = [0.0f64; 2];
    let mut max_bound = 0.0f64;
    let mut schema = "";
    for spec in &bundle.sources {
        ensure!(
            valid_id(&spec.id),
            "source id must be 1..=32 of [A-Za-z0-9_-]"
        );
        ensure!(
            ids.insert(spec.id.clone()),
            "duplicate source id {}",
            spec.id
        );
        ensure!(
            valid_name(&spec.name),
            "invalid source name for {}",
            spec.id
        );
        let mut sketch = match spec.format {
            SourceFormat::Svg => crate::inspect_svg(spec.text.as_bytes())?,
            SourceFormat::Dxf => crate::dxf_intake::inspect_dxf(spec.text.as_bytes(), tolerance)?,
            SourceFormat::Bundle => anyhow::bail!("bundles do not nest"),
        };
        let source_sha256 = sketch.source_sha256.clone();
        let gap = apply_gap_rules(&mut sketch);
        let (normalized, degenerate) = (gap.log, gap.degenerate);
        if let Some(keep) = &spec.contours {
            ensure!(
                !keep.is_empty(),
                "source {}: empty contour selection",
                spec.id
            );
            for want in keep {
                ensure!(
                    sketch.contours.iter().any(|c| &c.source_id == want),
                    "source {}: unknown contour {want}",
                    spec.id
                );
            }
            sketch.contours.retain(|c| keep.contains(&c.source_id));
        }
        let geometry = to_transmog(
            &sketch,
            tolerance,
            spec.placement_xy_down_mm,
            spec.layout.as_ref(),
        )?;
        max_bound = max_bound.max(geometry.max_bezier_chord_bound_mm);
        schema = sketch.schema;
        viewport = [
            viewport[0].max(sketch.viewport_mm[0]),
            viewport[1].max(sketch.viewport_mm[1]),
        ];
        for d in sketch.diagnostics.iter().chain(&geometry.diagnostics) {
            if !diagnostics.contains(d) {
                diagnostics.push(d.clone());
            }
        }
        let per = sketch.contours.len();
        let count = spec.layout.as_ref().map_or(1, |l| l.instances.len());
        ensure!(
            per > 0 && geometry.features.len() == per * count,
            "source {}: unexpected feature count",
            spec.id
        );
        let instances = (0..count)
            .map(|i| {
                let (min, max) = bbox_down(&geometry.features[i * per..(i + 1) * per])?;
                let (rotation_deg, scale, placement) = match &spec.layout {
                    Some(l) => {
                        let inst = &l.instances[i];
                        (inst.rotation_deg, inst.scale, inst.placement_xy_down_mm)
                    }
                    None => (0.0, 1.0, spec.placement_xy_down_mm),
                };
                Ok(InstanceRecord {
                    rotation_deg,
                    scale,
                    placement_xy_down_mm: placement,
                    bbox_min_xy_down_mm: min,
                    bbox_max_xy_down_mm: max,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let mut contour_ids = Vec::new();
        for (feature, id) in geometry.features.into_iter().zip(geometry.contour_identity) {
            let combined = format!("{}:{}", spec.id, id.source_id);
            contour_ids.push(combined.clone());
            features.push(feature);
            identity.push(ContourIdentity {
                source_id: combined,
                closed: id.closed,
            });
        }
        for c in sketch.contours {
            contours.push(crate::Contour {
                source_id: format!("{}:{}", spec.id, c.source_id),
                ..c
            });
        }
        records.push(SourceRecord {
            id: spec.id.clone(),
            name: spec.name.clone(),
            format: spec.format,
            source_sha256,
            viewport_mm: sketch.viewport_mm,
            layout_applied: spec.layout.is_some(),
            contour_ids,
            normalized,
            degenerate,
            instances,
        });
    }
    let source_sha256 = format!("{:x}", Sha256::digest(bytes));
    diagnostics.push("Multi-source bundle: contour ids are <source id>:<contour id>[#<copy>]; the planning sketch is the unplaced union of every source.".to_string());
    let sketch = Sketch {
        schema,
        source_sha256: source_sha256.clone(),
        units: "mm",
        coordinate_system: "svg_top_left_y_down",
        viewport_mm: viewport,
        machine_output_enabled: false,
        contours,
        diagnostics: diagnostics.clone(),
    };
    let geometry = Geometry {
        schema: "reversal.transmog-sketch-bridge.v1",
        source_sha256,
        coordinate_system: "xy_mm_top_left_y_negative_down",
        placement_xy_down_mm: [0.0, 0.0],
        layout: None,
        requested_curve_tolerance_mm: tolerance,
        max_bezier_chord_bound_mm: max_bound,
        features,
        contour_identity: identity,
        sources: Some(records),
        diagnostics,
    };
    Ok((sketch, geometry))
}

#[derive(Debug, Serialize)]
pub struct Integration {
    pub schema: &'static str,
    pub source_format: SourceFormat,
    pub planning: cutting::Job,
    pub transmog: Geometry,
    pub swarf: swarf_waterjet::Draft,
    pub machine_output_enabled: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Selection<'a> {
    pub model: Model,
    pub material: &'a str,
    pub units: Units,
    pub thickness: f64,
    /// An explicit user recipe replaces the catalog lookup. It is never merged
    /// into the catalog and is recorded (marked `user_defined`) in the job.
    pub user_material: Option<&'a UserMaterial>,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SourceFormat {
    Svg,
    Dxf,
    /// A [`SourceBundle`] JSON document holding several SVG/DXF sources.
    Bundle,
}

pub fn integrate_source(
    bytes: &[u8],
    format: SourceFormat,
    catalog: &Catalog,
    selection: Selection<'_>,
    tolerance: f64,
    placement: [f64; 2],
    layout: Option<&Layout>,
) -> Result<Integration> {
    let (sketch, geometry) = match format {
        SourceFormat::Bundle => {
            ensure!(
                placement == [0.0, 0.0] && layout.is_none(),
                "a source bundle carries placement and layout per source"
            );
            ensure!(
                tolerance.is_finite() && (0.000001..=1.0).contains(&tolerance),
                "curve tolerance must be 0.000001..1 mm"
            );
            bundle_geometry(bytes, tolerance)?
        }
        single => {
            let sketch = match single {
                SourceFormat::Svg => crate::inspect_svg(bytes)?,
                _ => crate::dxf_intake::inspect_dxf(bytes, tolerance)?,
            };
            let geometry = to_transmog(&sketch, tolerance, placement, layout)?;
            (sketch, geometry)
        }
    };
    let planning = cutting::plan_with(
        sketch,
        catalog,
        selection.model,
        selection.material,
        selection.units,
        selection.thickness,
        selection.user_material,
    )?;
    let contours = geometry
        .features
        .iter()
        .zip(&geometry.contour_identity)
        .map(|(f, id)| {
            let Feature::Sketch { profile, .. } = f else {
                unreachable!("adapter emits only sketches")
            };
            swarf_waterjet::Contour {
                source_id: id.source_id.clone(),
                closed: id.closed,
                profile: profile.clone(),
            }
        })
        .collect();
    let limits = &planning.machine_limits;
    let draft = swarf_waterjet::compile(swarf_waterjet::Request {
        material: planning.selected_row.material.clone(),
        thickness_mm: match selection.units {
            Units::Metric => selection.thickness,
            Units::Imperial => selection.thickness * 25.4,
        },
        feed_mm_min: planning.feed_mm_min,
        pierce_seconds: planning
            .selected_row
            .cut
            .as_ref()
            .context("missing cut")?
            .pierce_seconds,
        nominal_kerf_mm: planning.nominal_kerf_mm,
        cutting_area_width_depth_mm: [
            limits.cutting_area_depth_width_mm[1],
            limits.cutting_area_depth_width_mm[0],
        ],
        max_feed_mm_min: limits.max_gantry_mm_min,
        contours,
    })?;
    Ok(Integration {
        schema: "reversal.waterjet-integration.v1",
        source_format: format,
        planning,
        transmog: geometry,
        swarf: draft,
        machine_output_enabled: false,
    })
}

/// Rebuild both owner stages from original bytes, never trust serialized features
/// or operations as authority. Source selection and placement remain explicit.
pub fn verified_integration(
    original: &[u8],
    report: &[u8],
    catalog: &Catalog,
) -> Result<Integration> {
    ensure!(
        report.len() <= 64 * 1024 * 1024,
        "integration report exceeds 64 MiB bound"
    );
    let recorded: serde_json::Value = serde_json::from_slice(report)?;
    let row: cutting::Row = serde_json::from_value(recorded["planning"]["selected_row"].clone())?;
    let format: SourceFormat = serde_json::from_value(recorded["source_format"].clone())?;
    let tolerance = recorded["transmog"]["requested_curve_tolerance_mm"]
        .as_f64()
        .context("missing tolerance")?;
    let placement: [f64; 2] =
        serde_json::from_value(recorded["transmog"]["placement_xy_down_mm"].clone())?;
    // The applied layout is retained, so reopen expands the same copies from the
    // retained source and typed intent rather than trusting stored geometry.
    let layout: Option<Layout> = serde_json::from_value(recorded["transmog"]["layout"].clone())?;
    let user_material: Option<UserMaterial> =
        serde_json::from_value(recorded["planning"]["user_material"].clone())?;
    ensure!(
        user_material.is_some() == row.user_defined,
        "user-defined material marker differs from the recorded recipe"
    );
    let rebuilt = integrate_source(
        original,
        format,
        catalog,
        Selection {
            model: row.model,
            material: &row.material,
            units: row.units,
            thickness: row.thickness,
            user_material: user_material.as_ref(),
        },
        tolerance,
        placement,
        layout.as_ref(),
    )?;
    ensure!(
        crate::json_identity::equivalent(&recorded, &serde_json::to_value(&rebuilt)?),
        "integration differs from original/owner recomputation"
    );
    Ok(rebuilt)
}

pub fn verify_source(
    original: &[u8],
    report: &[u8],
    catalog: &Catalog,
) -> Result<serde_json::Value> {
    verified_integration(original, report, catalog)?;
    use sha2::{Digest, Sha256};
    Ok(
        serde_json::json!({"schema":"reversal.waterjet-integration-verification.v1","verified":true,
        "source_sha256":format!("{:x}",Sha256::digest(original)),"report_sha256":format!("{:x}",Sha256::digest(report)),"machine_output_enabled":false}),
    )
}
