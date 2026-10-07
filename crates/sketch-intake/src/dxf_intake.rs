//! Bounded planar DXF inspection using the established dxf parser.
use crate::{Contour, MAX_INPUT_BYTES, Segment, Sketch};
use anyhow::{Context, Result, bail, ensure};
use dxf::{Drawing, Point, Vector, entities::EntityType};
use sha2::{Digest, Sha256};
use std::{
    f64::consts::{PI, TAU},
    io::Cursor,
};

/// Require explicit units and supported ASCII ENTITIES before parsing. The
/// parser may skip unknown records; comparing counts prevents partial intake.
fn guard(bytes: &[u8]) -> Result<(usize, usize)> {
    ensure!(
        bytes.len() <= MAX_INPUT_BYTES,
        "DXF exceeds 1 MiB input limit"
    );
    let text =
        std::str::from_utf8(bytes).context("ASCII/UTF-8 DXF required; binary DXF unsupported")?;
    let lines: Vec<_> = text.lines().collect();
    ensure!(lines.len() % 2 == 0, "truncated DXF pair");
    let mut section = "";
    let mut pending = false;
    let mut count = 0;
    let mut units = 0;
    let mut eof = false;
    let mut layer_record = false;
    let mut viewport = false;
    let mut viewport_paper_space = false;
    let mut paper_viewports = 0;
    for pair in lines.as_chunks::<2>().0 {
        let code: i32 = pair[0].trim().parse().context("invalid DXF group code")?;
        let value = pair[1].trim();
        ensure!(!eof, "data after DXF EOF");
        if code == 0 && viewport {
            ensure!(viewport_paper_space, "model-space DXF viewport unsupported");
            paper_viewports += 1;
            viewport = false;
        }
        if code == 0 && value == "SECTION" {
            pending = true;
            continue;
        }
        if pending {
            ensure!(code == 2, "section name missing");
            section = value;
            pending = false;
            continue;
        }
        if code == 0 && value == "ENDSEC" {
            section = "";
            continue;
        }
        if code == 0 && value == "EOF" {
            eof = true;
            continue;
        }
        if section == "HEADER" && code == 9 && value == "$INSUNITS" {
            units += 1;
        }
        if section == "TABLES" {
            if code == 0 {
                layer_record = value == "LAYER";
            }
            if layer_record && code == 70 {
                let flags: i32 = value.parse()?;
                ensure!(flags & 3 == 0, "frozen DXF layers unsupported");
            }
            if layer_record && code == 62 {
                let color: i32 = value.parse()?;
                ensure!(color >= 0, "disabled DXF layers unsupported");
            }
        }
        if section == "ENTITIES" && code == 0 {
            ensure!(
                matches!(value, "LINE" | "ARC" | "CIRCLE" | "LWPOLYLINE" | "VIEWPORT"),
                "unsupported DXF entity {value}; partial intake refused"
            );
            if value == "VIEWPORT" {
                viewport = true;
                viewport_paper_space = false;
            } else {
                count += 1;
            }
            ensure!(count <= 10000, "DXF entity count bound");
        }
        if viewport && code == 67 {
            ensure!(
                !viewport_paper_space && value == "1",
                "invalid viewport paper-space declaration"
            );
            viewport_paper_space = true;
        }
    }
    ensure!(
        eof && units == 1 && count > 0,
        "DXF requires EOF, one explicit INSUNITS and supported entities"
    );
    Ok((count, paper_viewports))
}
fn normal(v: &Vector) -> Result<()> {
    ensure!(
        v.x == 0.0 && v.y == 0.0 && v.z == 1.0,
        "non-XY DXF extrusion unsupported"
    );
    Ok(())
}
fn point(p: &Point, scale: f64) -> Result<[f64; 2]> {
    ensure!(
        p.x.is_finite() && p.y.is_finite() && p.z == 0.0,
        "DXF must have finite XY geometry at Z=0"
    );
    let out = [p.x * scale, -p.y * scale];
    ensure!(out.iter().all(|v| v.is_finite()), "DXF scale overflow");
    Ok(out)
}
fn arc(
    center: [f64; 2],
    radius: f64,
    start: f64,
    sweep: f64,
    tolerance: f64,
    closed: bool,
) -> Result<Vec<Segment>> {
    ensure!(
        radius.is_finite()
            && radius > 0.0
            && start.is_finite()
            && sweep.is_finite()
            && sweep > 0.0
            && sweep <= TAU,
        "invalid DXF arc"
    );
    // Sagitta r*(1-cos(theta/2)) <= tolerance; cap to quarter turns.
    // Stable asin form avoids cancellation for fine tolerances.
    let step = (4.0 * (tolerance / (2.0 * radius)).min(0.5).sqrt().asin()).min(PI / 2.0);
    ensure!(step.is_finite() && step > 0.0, "invalid arc tolerance");
    let count = (sweep / step).ceil().max(1.0);
    ensure!(count <= 100000.0, "DXF arc exceeds segment bound");
    let count = count as usize;
    let at = |angle: f64| {
        [
            center[0] + radius * angle.cos(),
            center[1] - radius * angle.sin(),
        ]
    };
    let first = at(start);
    let mut out = vec![Segment::Move { to: first }];
    for i in 1..=count {
        out.push(Segment::Line {
            to: if closed && i == count {
                first
            } else {
                at(start + sweep * i as f64 / count as f64)
            },
        });
    }
    if closed {
        out.push(Segment::Close);
    }
    Ok(out)
}

pub fn inspect_dxf(bytes: &[u8], tolerance: f64) -> Result<Sketch> {
    ensure!(
        tolerance.is_finite() && (0.000001..=1.0).contains(&tolerance),
        "curve tolerance must be 0.000001..1 mm"
    );
    let (expected, paper_viewports) = guard(bytes)?;
    let drawing = Drawing::load(&mut Cursor::new(bytes))?;
    let scale = match drawing.header.default_drawing_units {
        dxf::enums::Units::Millimeters => 1.0,
        dxf::enums::Units::Inches => 25.4,
        dxf::enums::Units::Centimeters => 10.0,
        dxf::enums::Units::Meters => 1000.0,
        _ => bail!("DXF units are unitless or unsupported; scale must not be guessed"),
    };
    let entities: Vec<_> = drawing.entities().collect();
    ensure!(
        entities.len() == expected,
        "DXF parser dropped source entities"
    );
    let mut contours = Vec::new();
    let mut segments_count = 0;
    for (index, e) in entities.iter().enumerate() {
        ensure!(
            e.common.is_visible && !e.common.is_in_paper_space && e.common.elevation == 0.0,
            "hidden, paper-space or elevated DXF entities unsupported"
        );
        let (segments, closed) = match &e.specific {
            EntityType::Line(line) => {
                normal(&line.extrusion_direction)?;
                ensure!(line.thickness == 0.0, "thick DXF line unsupported");
                (
                    vec![
                        Segment::Move {
                            to: point(&line.p1, scale)?,
                        },
                        Segment::Line {
                            to: point(&line.p2, scale)?,
                        },
                    ],
                    false,
                )
            }
            EntityType::Arc(a) => {
                normal(&a.normal)?;
                ensure!(a.thickness == 0.0, "thick DXF arc unsupported");
                let sweep = (a.end_angle - a.start_angle).to_radians().rem_euclid(TAU);
                (
                    arc(
                        point(&a.center, scale)?,
                        a.radius * scale,
                        a.start_angle.to_radians(),
                        sweep,
                        tolerance,
                        false,
                    )?,
                    false,
                )
            }
            EntityType::Circle(c) => {
                normal(&c.normal)?;
                ensure!(c.thickness == 0.0, "thick DXF circle unsupported");
                (
                    arc(
                        point(&c.center, scale)?,
                        c.radius * scale,
                        0.0,
                        TAU,
                        tolerance,
                        true,
                    )?,
                    true,
                )
            }
            EntityType::LwPolyline(p) => {
                normal(&p.extrusion_direction)?;
                ensure!(
                    p.thickness == 0.0 && p.constant_width == 0.0 && p.vertices.len() >= 2,
                    "unsupported/empty DXF polyline"
                );
                let mut out = Vec::new();
                for (i, v) in p.vertices.iter().enumerate() {
                    ensure!(
                        v.bulge == 0.0 && v.starting_width == 0.0 && v.ending_width == 0.0,
                        "bulged or wide DXF polyline unsupported"
                    );
                    let to = point(&Point::new(v.x, v.y, 0.0), scale)?;
                    out.push(if i == 0 {
                        Segment::Move { to }
                    } else {
                        Segment::Line { to }
                    });
                }
                if p.is_closed() {
                    out.push(Segment::Close);
                }
                (out, p.is_closed())
            }
            _ => bail!("unsupported DXF entity"),
        };
        segments_count += segments.len();
        ensure!(segments_count <= 100000, "DXF total segment bound");
        contours.push(Contour {
            source_id: format!("dxf-entity-{index}-layer-{}", e.common.layer),
            closed,
            segments,
        });
    }
    ensure!(!contours.is_empty(), "DXF has no supported cut geometry");
    Ok(Sketch {
        schema: "reversal.sketch-preview.v1",
        source_sha256: format!("{:x}", Sha256::digest(bytes)),
        units: "mm",
        coordinate_system: "svg_top_left_y_down",
        viewport_mm: [0.0, 0.0],
        machine_output_enabled: false,
        contours,
        diagnostics: vec![
            format!(
                "DXF INSUNITS scale {scale} mm/unit; CAD Y reflected about original origin; viewport unspecified, explicit stock placement required."
            ),
            format!(
                "Circular chords have at most {tolerance} mm sagitta error. Separate entities remain separate; no snapping, joining or hole classification."
            ),
            format!(
                "{paper_viewports} paper-space VIEWPORT records explicitly classified as layout metadata. Block INSERTs, SPLINEs, bulged polylines and non-planar/hidden geometry fail; unknown records are never silently skipped."
            ),
        ],
    })
}
