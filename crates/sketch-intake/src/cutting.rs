//! Source-pinned published WAZER tables and offline job estimates.
use crate::{Segment, Sketch};
use anyhow::{Context, Result, ensure};
use kurbo::{CubicBez, ParamCurveArclen, QuadBez};
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const MATERIAL_URL: &str = "https://wazer.com/materials/materials-specs/";
const SPEC_URL: &str = "https://wazer.com/waterjets/tech-specs/";
const MAX_HTML: usize = 2 * 1024 * 1024;
const DATA: &[u8] = include_bytes!("../../../wazer/cutting-tables.json");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Model {
    Pro,
    Desktop,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Units {
    Metric,
    Imperial,
}
impl Units {
    fn css(self) -> &'static str {
        match self {
            Self::Metric => ".unit-metric",
            Self::Imperial => ".unit-imperial",
        }
    }
    fn mm(self, value: f64) -> f64 {
        match self {
            Self::Metric => value,
            Self::Imperial => value * 25.4,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub url: String,
    pub checked_on: String,
    pub sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Cut {
    pub feed: f64,
    pub pierce_seconds: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub source_row: usize,
    pub material: String,
    pub model: Model,
    pub units: Units,
    pub thickness: f64,
    pub thickness_label: String,
    pub maximum_marked: bool,
    pub cut: Option<Cut>,
    pub warnings: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub label: String,
    pub pro: String,
    pub desktop: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub model: Model,
    pub units: Units,
    pub nominal_kerf_mm: f64,
    pub cutting_area_depth_width_mm: [f64; 2],
    pub max_gantry_mm_min: f64,
    pub abrasive_kg_min: f64,
    pub hopper_capacity_kg: f64,
    pub continuous_cut_minutes: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema: String,
    pub materials_source: Source,
    pub specs_source: Source,
    pub rows: Vec<Row>,
    pub limits: Vec<Limits>,
    pub specifications: Vec<Spec>,
}

fn selector(value: &str) -> Selector {
    Selector::parse(value).expect("constant CSS selector")
}
fn text(element: ElementRef<'_>) -> String {
    element
        .text()
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn one<'a>(element: ElementRef<'a>, css: &str) -> Result<ElementRef<'a>> {
    let found: Vec<_> = element.select(&selector(css)).collect();
    ensure!(found.len() == 1, "expected one {css}, got {}", found.len());
    Ok(found[0])
}
fn unit_text(element: ElementRef<'_>, units: Units) -> Result<String> {
    Ok(text(one(element, units.css())?))
}
fn positive(value: f64) -> Result<f64> {
    ensure!(
        value.is_finite() && value > 0.0,
        "value must be positive and finite"
    );
    Ok(value)
}
fn first_number(label: &str) -> Result<f64> {
    let end = label
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(label.len());
    positive(label[..end].parse().context("invalid numeric label")?)
}
fn feed(label: &str, units: Units) -> Result<f64> {
    let suffix = match units {
        Units::Metric => "mmpm",
        Units::Imperial => "ipm",
    };
    positive(
        label
            .strip_suffix(suffix)
            .context("unexpected feed units")?
            .trim()
            .parse()?,
    )
}
fn seconds(label: &str) -> Result<f64> {
    let label = label.split_whitespace().collect::<String>();
    if let Some(n) = label.strip_suffix("sec") {
        return positive(n.parse()?);
    }
    let (minutes, rest) = if let Some((m, s)) = label.split_once('m') {
        (m.parse::<f64>()?, s)
    } else {
        (0.0, label.as_str())
    };
    let sec = if rest.is_empty() {
        0.0
    } else {
        rest.strip_suffix('s')
            .context("invalid pierce duration")?
            .parse::<f64>()?
    };
    ensure!(
        minutes.is_finite() && minutes >= 0.0 && sec.is_finite() && sec >= 0.0,
        "invalid pierce duration"
    );
    positive(minutes * 60.0 + sec)
}
fn source(bytes: &[u8], url: &str, date: &str) -> Source {
    Source {
        url: url.into(),
        checked_on: date.into(),
        sha256: format!("{:x}", Sha256::digest(bytes)),
    }
}

pub fn import_published(materials: &[u8], specs: &[u8], date: &str) -> Result<Catalog> {
    ensure!(
        materials.len() <= MAX_HTML && specs.len() <= MAX_HTML,
        "HTML exceeds 2 MiB bound"
    );
    ensure!(
        date.len() == 10
            && date.as_bytes()[4] == b'-'
            && date.as_bytes()[7] == b'-'
            && date
                .bytes()
                .enumerate()
                .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit()),
        "date must be YYYY-MM-DD"
    );
    let doc = Html::parse_document(std::str::from_utf8(materials)?);
    let mut rows = Vec::new();
    let mut source_row = 0;
    for table in doc.select(&selector(".materials-table__list-item-table table")) {
        // Verify column identity before reading positional model cells.
        let headers: Vec<_> = table
            .select(&selector(".materials-table__header-product-cell"))
            .map(text)
            .collect();
        ensure!(
            !headers.is_empty()
                && headers.len() % 2 == 0
                && headers
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .all(|h| h[0].contains("Pro") && h[1].contains("Desktop")),
            "model headers changed: {headers:?}"
        );
        let mut material = String::new();
        for tr in table.select(&selector("tr")) {
            let thickness = tr
                .select(&selector("td.materials-table__cell-thickness"))
                .next();
            let Some(thickness) = thickness else { continue };
            let label = text(one(tr, "th.materials-table__cell-material")?);
            if !label.is_empty() {
                material = label;
            }
            ensure!(!material.is_empty(), "material name missing");
            source_row += 1;
            ensure!(source_row <= 2500, "table exceeds row bound");
            let rates: Vec<_> = tr
                .select(&selector("td.materials-table__cell-cut-rate"))
                .collect();
            let pierces: Vec<_> = tr
                .select(&selector("td.materials-table__cell-cut-speed"))
                .collect();
            ensure!(
                rates.len() == pierces.len() && (rates.len() == 1 || rates.len() == 2),
                "rate/pierce columns changed"
            );
            if rates.len() == 1 {
                ensure!(
                    text(one(tr, "td.materials-table__cell-too-thick")?)
                        .contains("Too thick for Desktop"),
                    "missing Desktop column"
                );
            }
            let inch = first_number(&unit_text(thickness, Units::Imperial)?)? * 25.4;
            let mm = first_number(&unit_text(thickness, Units::Metric)?)?;
            let ambiguous = inch / mm > 2.0 || mm / inch > 2.0;
            for units in [Units::Metric, Units::Imperial] {
                let thickness_label = unit_text(thickness, units)?;
                for (i, model) in [Model::Pro, Model::Desktop].into_iter().enumerate() {
                    let cut = if i < rates.len() {
                        Some(Cut {
                            feed: feed(&unit_text(rates[i], units)?, units)?,
                            pierce_seconds: seconds(&unit_text(pierces[i], units)?)?,
                        })
                    } else {
                        None
                    };
                    rows.push(Row {source_row,material:material.clone(),model,units,thickness:first_number(&thickness_label)?,maximum_marked:thickness_label.contains(match model{Model::Pro=>"Max Pro",Model::Desktop=>"Max Desktop"}),thickness_label:thickness_label.clone(),cut,warnings:if ambiguous{vec!["paired thickness labels differ by more than a factor of two; source row requires correction".into()]}else{vec![]}});
                }
            }
        }
    }
    ensure!(!rows.is_empty(), "no published cutting rows found");
    let doc = Html::parse_document(std::str::from_utf8(specs)?);
    let mut specifications = Vec::new();
    for table in doc.select(&selector("figure.is-style-specs table")) {
        for tr in table.select(&selector("tr")) {
            let cells: Vec<_> = tr
                .children()
                .filter_map(ElementRef::wrap)
                .filter(|e| matches!(e.value().name(), "th" | "td"))
                .collect();
            if cells.len() == 3 {
                let label = text(cells[0]);
                if !matches!(
                    label.as_str(),
                    "Cutting Area (D x W)"
                        | "Cut Bed Size (D x W)"
                        | "Kerf (width of cut)"
                        | "Gantry Max Speed"
                        | "Gantry Positional Accuracy"
                        | "Abrasive Rate"
                        | "Abrasive Hopper Capacity"
                        | "Abrasive Material"
                        | "Continuous Cutting Time"
                        | "Orifice Diameter"
                        | "Mixing Tube Diameter (internal)"
                ) {
                    continue;
                }
                specifications.push(Spec {
                    label,
                    pro: text(cells[1]),
                    desktop: text(cells[2]),
                });
            }
        }
    }
    let mut limits = Vec::new();
    for model in [Model::Pro, Model::Desktop] {
        let get = |label: &str| -> Result<&str> {
            let matches: Vec<_> = specifications.iter().filter(|s| s.label == label).collect();
            ensure!(!matches.is_empty(), "missing spec {label}");
            ensure!(
                matches
                    .iter()
                    .all(|s| s.pro == matches[0].pro && s.desktop == matches[0].desktop),
                "conflicting repeated spec {label}"
            );
            Ok(match model {
                Model::Pro => &matches[0].pro,
                Model::Desktop => &matches[0].desktop,
            })
        };
        for units in [Units::Metric, Units::Imperial] {
            let scalar_unit = match units {
                Units::Metric => "mm",
                Units::Imperial => "in",
            };
            let kerf = numeric_before(get("Kerf (width of cut)")?, scalar_unit)?;
            let area = dimensions_before(get("Cutting Area (D x W)")?, scalar_unit)?;
            let max = numeric_before(
                get("Gantry Max Speed")?,
                match units {
                    Units::Metric => "mm/min",
                    Units::Imperial => "IPM",
                },
            )?;
            let mass_unit = match units {
                Units::Metric => "kg",
                Units::Imperial => "lb",
            };
            let mass_scale = match units {
                Units::Metric => 1.0,
                Units::Imperial => 0.45359237,
            };
            let abrasive =
                numeric_before(get("Abrasive Rate")?, &format!("{mass_unit}/min"))? * mass_scale;
            let hopper = numeric_before(get("Abrasive Hopper Capacity")?, mass_unit)? * mass_scale;
            let continuous = numeric_before(get("Continuous Cutting Time")?, "min")?;
            limits.push(Limits {
                model,
                units,
                nominal_kerf_mm: units.mm(kerf),
                cutting_area_depth_width_mm: [units.mm(area[0]), units.mm(area[1])],
                max_gantry_mm_min: units.mm(max),
                abrasive_kg_min: abrasive,
                hopper_capacity_kg: hopper,
                continuous_cut_minutes: continuous,
            });
        }
    }
    let catalog = Catalog {
        schema: "reversal.wazer-cutting-tables.v1".into(),
        materials_source: source(materials, MATERIAL_URL, date),
        specs_source: source(specs, SPEC_URL, date),
        rows,
        limits,
        specifications,
    };
    catalog.validate()?;
    Ok(catalog)
}
fn numeric_before(label: &str, unit: &str) -> Result<f64> {
    let t: Vec<_> = label.split_whitespace().collect();
    let i = t
        .iter()
        .position(|x| *x == unit)
        .context("spec units missing")?;
    ensure!(i > 0, "missing spec value");
    positive(t[i - 1].replace(',', "").parse()?)
}
fn dimensions_before(label: &str, unit: &str) -> Result<[f64; 2]> {
    let t: Vec<_> = label.split_whitespace().collect();
    let i = t
        .iter()
        .position(|x| *x == unit)
        .context("area units missing")?;
    ensure!(i >= 3 && t[i - 2] == "x", "invalid dimensions");
    Ok([positive(t[i - 3].parse()?)?, positive(t[i - 1].parse()?)?])
}

impl Catalog {
    pub fn canonical_sha256(&self) -> Result<String> {
        Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(self)?)))
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.schema == "reversal.wazer-cutting-tables.v1",
            "unknown table schema"
        );
        for s in [&self.materials_source, &self.specs_source] {
            ensure!(
                s.sha256.len() == 64 && s.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
                "invalid source digest"
            );
        }
        ensure!(
            self.materials_source.url == MATERIAL_URL && self.specs_source.url == SPEC_URL,
            "wrong source identity"
        );
        ensure!(
            !self.rows.is_empty() && self.rows.len() <= 10000,
            "invalid row count"
        );
        let mut keys = BTreeSet::new();
        for r in &self.rows {
            ensure!(
                !r.material.is_empty() && r.source_row > 0,
                "invalid row identity"
            );
            positive(r.thickness)?;
            ensure!(
                keys.insert(format!(
                    "{}:{:?}:{:?}:{}",
                    r.material.to_lowercase(),
                    r.model,
                    r.units,
                    r.thickness
                )),
                "duplicate row selection key"
            );
            if let Some(c) = &r.cut {
                positive(c.feed)?;
                positive(c.pierce_seconds)?;
            }
        }
        let mut groups: BTreeMap<usize, Vec<&Row>> = BTreeMap::new();
        for r in &self.rows {
            groups.entry(r.source_row).or_default().push(r);
        }
        for (index, (id, rows)) in groups.iter().enumerate() {
            ensure!(
                *id == index + 1 && rows.len() == 4,
                "missing source row or model/unit variant"
            );
            let mut variants = BTreeSet::new();
            for r in rows {
                ensure!(
                    r.material == rows[0].material
                        && variants.insert(format!("{:?}:{:?}", r.model, r.units)),
                    "inconsistent source row variants"
                );
            }
            let mm = rows
                .iter()
                .find(|r| r.units == Units::Metric)
                .context("missing metric variant")?
                .thickness;
            let inch = rows
                .iter()
                .find(|r| r.units == Units::Imperial)
                .context("missing imperial variant")?
                .thickness
                * 25.4;
            let anomalous = inch / mm > 2.0 || mm / inch > 2.0;
            ensure!(
                !anomalous || rows.iter().all(|r| !r.warnings.is_empty()),
                "unflagged source thickness anomaly"
            );
        }
        ensure!(self.limits.len() == 4, "expected all model/unit limits");
        let mut keys = BTreeSet::new();
        for l in &self.limits {
            ensure!(
                keys.insert(format!("{:?}:{:?}", l.model, l.units)),
                "duplicate limits"
            );
            positive(l.nominal_kerf_mm)?;
            positive(l.max_gantry_mm_min)?;
            positive(l.abrasive_kg_min)?;
            positive(l.hopper_capacity_kg)?;
            positive(l.continuous_cut_minutes)?;
            for v in l.cutting_area_depth_width_mm {
                positive(v)?;
            }
        }
        Ok(())
    }
    pub fn select(
        &self,
        model: Model,
        material: &str,
        units: Units,
        thickness: f64,
    ) -> Result<&Row> {
        self.validate()?;
        positive(thickness)?;
        let matches: Vec<_> = self
            .rows
            .iter()
            .filter(|r| {
                r.model == model
                    && r.units == units
                    && r.material.eq_ignore_ascii_case(material)
                    && (r.thickness - thickness).abs() < 1e-9
            })
            .collect();
        ensure!(
            matches.len() == 1,
            "no exact published material/model/thickness row; interpolation and alloy aliasing are disabled"
        );
        let row = matches[0];
        ensure!(
            row.warnings.is_empty(),
            "source row has unresolved thickness anomaly"
        );
        ensure!(
            row.cut.is_some(),
            "published row is too thick for this machine"
        );
        Ok(row)
    }
}
pub fn bundled() -> Result<Catalog> {
    let acquisition: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../wazer/cutting-data-acquisition.json"
    ))?;
    let digest = format!("{:x}", Sha256::digest(DATA));
    ensure!(
        acquisition["normalized"]["sha256"].as_str() == Some(digest.as_str()),
        "bundled table bytes differ from acquisition receipt"
    );
    let catalog: Catalog = serde_json::from_slice(DATA)?;
    catalog.validate()?;
    for source in [&catalog.materials_source, &catalog.specs_source] {
        let matches: Vec<_> = acquisition["sources"]
            .as_array()
            .context("missing acquisition sources")?
            .iter()
            .filter(|s| {
                s["url"].as_str() == Some(source.url.as_str())
                    && s["sha256"].as_str() == Some(source.sha256.as_str())
            })
            .collect();
        ensure!(
            matches.len() == 1
                && acquisition["checked_on"].as_str() == Some(source.checked_on.as_str()),
            "table source provenance differs from acquisition"
        );
    }
    Ok(catalog)
}

#[derive(Debug, Serialize)]
pub struct Job {
    pub schema: &'static str,
    pub sketch: Sketch,
    pub tables_canonical_sha256: String,
    pub selected_row: Row,
    pub machine_limits: Limits,
    pub feed_mm_min: f64,
    pub feed_ipm: f64,
    pub nominal_kerf_mm: f64,
    pub nominal_side_offset_mm: f64,
    pub raw_centerline_length_mm: f64,
    pub raw_contour_pierces: usize,
    pub nominal_cut_and_pierce_seconds: f64,
    pub nominal_abrasive_kg: f64,
    pub machine_output_enabled: bool,
    pub blockers: Vec<String>,
}
pub fn plan(
    sketch: Sketch,
    catalog: &Catalog,
    model: Model,
    material: &str,
    units: Units,
    thickness: f64,
) -> Result<Job> {
    ensure!(
        sketch.schema == "reversal.sketch-preview.v1"
            && sketch.units == "mm"
            && sketch.coordinate_system == "svg_top_left_y_down"
            && !sketch.machine_output_enabled,
        "unsupported sketch coordinate contract"
    );
    let row = catalog.select(model, material, units, thickness)?.clone();
    let tables_canonical_sha256 = catalog.canonical_sha256()?;
    let limits = catalog
        .limits
        .iter()
        .find(|l| l.model == model && l.units == units)
        .context("missing machine limits")?
        .clone();
    let cut = row.cut.as_ref().context("missing cut settings")?;
    let feed_mm_min = units.mm(cut.feed);
    let mut length = 0.0;
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for c in &sketch.contours {
        let mut at = [0.0; 2];
        let mut start = [0.0; 2];
        for s in &c.segments {
            let points: Vec<[f64; 2]> = match s {
                Segment::Move { to } | Segment::Line { to } => vec![*to],
                Segment::Quadratic { control, to } => vec![*control, *to],
                Segment::Cubic { controls, to } => vec![controls[0], controls[1], *to],
                Segment::Close => vec![start],
            };
            for p in points {
                for i in 0..2 {
                    ensure!(p[i].is_finite(), "nonfinite geometry");
                    min[i] = min[i].min(p[i]);
                    max[i] = max[i].max(p[i]);
                }
            }
            match s {
                Segment::Move { to } => {
                    at = *to;
                    start = *to;
                }
                Segment::Line { to } => {
                    length += (at[0] - to[0]).hypot(at[1] - to[1]);
                    at = *to;
                }
                Segment::Quadratic { control, to } => {
                    length +=
                        QuadBez::new((at[0], at[1]), (control[0], control[1]), (to[0], to[1]))
                            .arclen(0.001);
                    at = *to;
                }
                Segment::Cubic { controls, to } => {
                    length += CubicBez::new(
                        (at[0], at[1]),
                        (controls[0][0], controls[0][1]),
                        (controls[1][0], controls[1][1]),
                        (to[0], to[1]),
                    )
                    .arclen(0.001);
                    at = *to;
                }
                Segment::Close => {
                    length += (at[0] - start[0]).hypot(at[1] - start[1]);
                    at = start;
                }
            }
        }
    }
    positive(length)?;
    let mut blockers=vec!["Geometry/topology, cut side, leads, tabs, quality/corner policy and WAZER postprocessor still require validation; public table feed has no specified cut quality.".into(),"Kerf is the published nominal machine value, not an exact-unit measurement or material-specific calibration.".into()];
    if sketch.contours.iter().any(|c| !c.closed) {
        blockers
            .push("Open contours require explicit cutting intent; no automatic closure.".into());
    }
    if min[0] < 0.0
        || min[1] < 0.0
        || max[0] > limits.cutting_area_depth_width_mm[1]
        || max[1] > limits.cutting_area_depth_width_mm[0]
    {
        blockers.push("Conservative control-point bounds exceed the published cutting area; stock placement required.".into());
    }
    if feed_mm_min > limits.max_gantry_mm_min {
        blockers.push("Published table feed exceeds published gantry maximum (possibly rounding); no silent clamping.".into());
    }
    let pierces = sketch.contours.len();
    let seconds = length / feed_mm_min * 60.0 + pierces as f64 * cut.pierce_seconds;
    ensure!(seconds.is_finite(), "estimate overflow");
    let abrasive = seconds / 60.0 * limits.abrasive_kg_min;
    ensure!(abrasive.is_finite(), "abrasive estimate overflow");
    if abrasive > limits.hopper_capacity_kg || seconds / 60.0 > limits.continuous_cut_minutes {
        blockers.push("Nominal estimate exceeds published hopper capacity or continuous cutting time; refill planning required.".into());
    }
    Ok(Job {
        schema: "reversal.waterjet-job-estimate.v1",
        sketch,
        tables_canonical_sha256,
        selected_row: row,
        nominal_kerf_mm: limits.nominal_kerf_mm,
        nominal_side_offset_mm: limits.nominal_kerf_mm / 2.0,
        machine_limits: limits,
        feed_mm_min,
        feed_ipm: feed_mm_min / 25.4,
        raw_centerline_length_mm: length,
        raw_contour_pierces: pierces,
        nominal_cut_and_pierce_seconds: seconds,
        nominal_abrasive_kg: abrasive,
        machine_output_enabled: false,
        blockers,
    })
}

/// Recompute a retained report from original SVG bytes and the current table snapshot.
/// Hashes and claimed measurements inside the supplied report are never authority.
pub fn verify_job(svg: &[u8], report: &[u8], catalog: &Catalog) -> Result<serde_json::Value> {
    ensure!(
        report.len() <= 32 * 1024 * 1024,
        "report exceeds 32 MiB bound"
    );
    let recorded: serde_json::Value = serde_json::from_slice(report)?;
    let selection: Row = serde_json::from_value(
        recorded
            .get("selected_row")
            .context("missing selected row")?
            .clone(),
    )?;
    let recomputed = plan(
        crate::inspect_svg(svg)?,
        catalog,
        selection.model,
        &selection.material,
        selection.units,
        selection.thickness,
    )?;
    ensure!(
        crate::json_identity::equivalent(&recorded, &serde_json::to_value(&recomputed)?),
        "report differs from independently recomputed geometry/table selection/estimate"
    );
    Ok(
        serde_json::json!({"schema":"reversal.waterjet-job-verification.v1","verified":true,"report_sha256":format!("{:x}",Sha256::digest(report)),"source_sha256":recomputed.sketch.source_sha256,"tables_canonical_sha256":recomputed.tables_canonical_sha256,"machine_output_enabled":false}),
    )
}
