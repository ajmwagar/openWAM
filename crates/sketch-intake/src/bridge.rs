//! Explicit adapter to the Transmog sketch contract and Swarf waterjet owner.
use crate::{
    Segment, Sketch,
    cutting::{self, Catalog, Model, Units},
};
use anyhow::{Context, Result, ensure};
use kurbo::{CubicBez, ParamCurve, Point};
use serde::Serialize;
use transmog_core::{
    geometry::Point2,
    ir::{Feature, PlaneRef, SketchSegment},
};

const MAX_LINES: usize = 100_000;

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
    pub requested_curve_tolerance_mm: f64,
    pub max_bezier_chord_bound_mm: f64,
    pub features: Vec<Feature>,
    pub contour_identity: Vec<ContourIdentity>,
    pub diagnostics: Vec<String>,
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

/// Reflect Y to CAD XY and apply only the explicitly supplied stock placement.
pub fn to_transmog(sketch: &Sketch, tolerance: f64, placement: [f64; 2]) -> Result<Geometry> {
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
    let point = |p: [f64; 2]| Point::new(p[0] + placement[0], -p[1] - placement[1]);
    let mut flatten = Flatten {
        tolerance,
        max_bound: 0.0,
        count: 0,
    };
    let mut features = Vec::new();
    let mut identity = Vec::new();
    for c in &sketch.contours {
        let mut at = None;
        let mut start = None;
        let mut profile = Vec::new();
        for segment in &c.segments {
            match segment {
                Segment::Move { to } => {
                    ensure!(at.is_none(), "multiple moves in contour");
                    at = Some(point(*to));
                    start = at;
                }
                Segment::Line { to } => {
                    let next = point(*to);
                    flatten.line(at.context("missing contour start")?, next, &mut profile)?;
                    at = Some(next);
                }
                Segment::Quadratic { control, to } => {
                    let a = at.context("missing contour start")?;
                    let b = point(*to);
                    let q = point(*control);
                    flatten.cubic(
                        CubicBez::new(a, a + (q - a) * (2.0 / 3.0), b + (q - b) * (2.0 / 3.0), b),
                        0,
                        &mut profile,
                    )?;
                    at = Some(b);
                }
                Segment::Cubic { controls, to } => {
                    let next = point(*to);
                    flatten.cubic(
                        CubicBez::new(
                            at.context("missing contour start")?,
                            point(controls[0]),
                            point(controls[1]),
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
        features.push(Feature::Sketch {
            plane: PlaneRef::Xy,
            profile,
        });
        identity.push(ContourIdentity {
            source_id: c.source_id.clone(),
            closed: c.closed,
        });
    }
    ensure!(!features.is_empty(), "empty sketch");
    Ok(Geometry{schema:"reversal.transmog-sketch-bridge.v1",source_sha256:sketch.source_sha256.clone(),
        coordinate_system:"xy_mm_top_left_y_negative_down",placement_xy_down_mm:placement,
        requested_curve_tolerance_mm:tolerance,max_bezier_chord_bound_mm:flatten.max_bound,
        features,contour_identity:identity,diagnostics:vec![
            "Bezier bound is relative to normalized SVG curves; source SVG arc normalization and float32 transforms are additional upstream errors.".into(),
            "Each contour remains separate. No endpoint snapping, hole classification, topology certification or kerf offset is performed.".into(),
        ]})
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
}

#[derive(Clone, Copy, Debug, Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceFormat {
    Svg,
    Dxf,
}

pub fn integrate_source(
    bytes: &[u8],
    format: SourceFormat,
    catalog: &Catalog,
    selection: Selection<'_>,
    tolerance: f64,
    placement: [f64; 2],
) -> Result<Integration> {
    let sketch = match format {
        SourceFormat::Svg => crate::inspect_svg(bytes)?,
        SourceFormat::Dxf => crate::dxf_intake::inspect_dxf(bytes, tolerance)?,
    };
    let geometry = to_transmog(&sketch, tolerance, placement)?;
    let planning = cutting::plan(
        sketch,
        catalog,
        selection.model,
        selection.material,
        selection.units,
        selection.thickness,
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
    let rebuilt = integrate_source(
        original,
        format,
        catalog,
        Selection {
            model: row.model,
            material: &row.material,
            units: row.units,
            thickness: row.thickness,
        },
        tolerance,
        placement,
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
