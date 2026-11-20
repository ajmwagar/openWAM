//! Typed model of a WAM 2.4 `.wzr` project file.
//!
//! Field order in every struct is the order WAM writes (see `docs/wzr-format.md`);
//! serialisation relies on it for byte-identical write-back. Every struct denies
//! unknown keys and every string enum rejects unobserved values, so a schema
//! feature the samples do not cover fails loudly instead of being guessed.

use serde::{Deserialize, Deserializer, Serialize};

/// A field that is always present but may be JSON `null`.
fn nullable<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    /// Absent in files not written by WAM itself (e.g. the 2026-11-08 prototype).
    #[serde(
        rename = "wamVersion",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub wam_version: Option<String>,
    #[serde(rename = "fileList")]
    pub file_list: FileList,
    #[serde(rename = "selectedMaterial")]
    pub selected_material: SelectedMaterial,
    pub offsets: Vec<Offset>,
    pub tabs: Vec<Tab>,
    #[serde(rename = "cutQuality")]
    pub cut_quality: Quality,
    #[serde(rename = "cutOrder", default, skip_serializing_if = "Option::is_none")]
    pub cut_order: Option<CutOrder>,
    #[serde(
        rename = "displayUnit",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub display_unit: Option<LengthUnit>,
    pub placements: Vec<Placement>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileList {
    pub files: Vec<FileEntry>,
    #[serde(rename = "fileIDCounter")]
    pub file_id_counter: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileEntry {
    pub filename: String,
    #[serde(rename = "fileID")]
    pub file_id: u64,
    pub units: LengthUnit,
    #[serde(rename = "svgString")]
    pub svg_string: String,
    #[serde(rename = "splitGroups")]
    pub split_groups: bool,
    /// One inner list per file; one group ID per placed instance.
    #[serde(rename = "groupInstances")]
    pub group_instances: Vec<Vec<u64>>,
    pub instances: u64,
    #[serde(rename = "fileBB")]
    pub file_bb: BBox,
}

/// WAM bounding box, inches. `min/max/width/height` may be `null` (lead
/// polylines); the `original*` quartet is absent in prototype-written files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BBox {
    #[serde(rename = "maxX", deserialize_with = "nullable")]
    pub max_x: Option<f64>,
    #[serde(rename = "maxY", deserialize_with = "nullable")]
    pub max_y: Option<f64>,
    #[serde(rename = "minX", deserialize_with = "nullable")]
    pub min_x: Option<f64>,
    #[serde(rename = "minY", deserialize_with = "nullable")]
    pub min_y: Option<f64>,
    #[serde(deserialize_with = "nullable")]
    pub width: Option<f64>,
    #[serde(deserialize_with = "nullable")]
    pub height: Option<f64>,
    #[serde(
        rename = "originalMinX",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub original_min_x: Option<f64>,
    #[serde(
        rename = "originalMinY",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub original_min_y: Option<f64>,
    #[serde(
        rename = "originalMaxX",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub original_max_x: Option<f64>,
    #[serde(
        rename = "originalMaxY",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub original_max_y: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectedMaterial {
    pub material: Material,
    pub quality: Quality,
    #[serde(rename = "abrasiveMode")]
    pub abrasive_mode: AbrasiveMode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Material {
    #[serde(rename = "userDef")]
    pub user_def: bool,
    #[serde(rename = "uId", deserialize_with = "nullable")]
    pub u_id: Option<u64>,
    pub name: String,
    pub units: LengthUnit,
    pub category: String,
    pub thickness: MathUnit,
    #[serde(rename = "speedMode")]
    pub speed_mode: Rates,
    #[serde(rename = "ecoMode")]
    pub eco_mode: Rates,
    #[serde(rename = "pierceTime")]
    pub pierce_time: MathUnit,
    #[serde(rename = "defaultTabWidth")]
    pub default_tab_width: MathUnit,
    #[serde(rename = "defaultLeadDist")]
    pub default_lead_dist: MathUnit,
    #[serde(rename = "machineType")]
    pub machine_type: MachineType,
    /// Present for `pro` in the WAM samples, absent for `desktop`.
    #[serde(rename = "maxTab", default, skip_serializing_if = "Option::is_none")]
    pub max_tab: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rates {
    pub name: String,
    #[serde(rename = "abrasiveRate")]
    pub abrasive_rate: MathUnit,
    #[serde(rename = "roughRate")]
    pub rough_rate: MathUnit,
    #[serde(rename = "mediumRate")]
    pub medium_rate: MathUnit,
    #[serde(rename = "fineRate")]
    pub fine_rate: MathUnit,
}

/// A serialised math.js `Unit`: `{"mathjs":"Unit","value":…,"unit":"…","fixPrefix":false}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MathUnit {
    pub mathjs: MathJsTag,
    pub value: f64,
    pub unit: String,
    #[serde(rename = "fixPrefix")]
    pub fix_prefix: bool,
}

impl MathUnit {
    pub fn new(value: f64, unit: &str) -> Self {
        Self {
            mathjs: MathJsTag::Unit,
            value,
            unit: unit.to_owned(),
            fix_prefix: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MathJsTag {
    Unit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Offset {
    #[serde(rename = "groupID")]
    pub group_id: u64,
    #[serde(rename = "type")]
    pub kind: OffsetType,
    /// Inches. 0.019 in every WAM-written sample.
    pub distance: f64,
    #[serde(rename = "fileID")]
    pub file_id: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tab {
    #[serde(rename = "groupID")]
    pub group_id: u64,
    #[serde(rename = "fileID", default, skip_serializing_if = "Option::is_none")]
    pub file_id: Option<u64>,
    #[serde(rename = "type")]
    pub kind: TabType,
    #[serde(rename = "tabCount")]
    pub tab_count: u64,
    #[serde(rename = "tabSize")]
    pub tab_size: f64,
    #[serde(rename = "enableLeads")]
    pub enable_leads: bool,
    #[serde(rename = "cutType", default, skip_serializing_if = "Option::is_none")]
    pub cut_type: Option<CutType>,
    #[serde(
        rename = "scoredTabSpeedMultiplier",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub scored_tab_speed_multiplier: Option<f64>,
    /// WAM-computed cache: one inner list per tab set, inches, RSTBB-centred.
    #[serde(rename = "tabLocations")]
    pub tab_locations: Vec<Vec<TabLocation>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TabLocation {
    #[serde(rename = "tabStart")]
    pub tab_start: PathPoint,
    #[serde(rename = "tabEnd")]
    pub tab_end: PathPoint,
    #[serde(rename = "tabID")]
    pub tab_id: u64,
    #[serde(rename = "tabSize")]
    pub tab_size: f64,
    #[serde(rename = "cutType")]
    pub cut_type: CutType,
    #[serde(rename = "scoredTabSpeedMultiplier")]
    pub scored_tab_speed_multiplier: f64,
}

/// A point on a cut path. Tab ends carry `pathSegment` and an optional lead;
/// lead polyline points omit `pathSegment` and always carry `lead: null`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathPoint {
    pub x: f64,
    pub y: f64,
    #[serde(rename = "pathDistance")]
    pub path_distance: f64,
    #[serde(
        rename = "pathSegment",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub path_segment: Option<u64>,
    #[serde(deserialize_with = "nullable")]
    pub lead: Option<Box<Lead>>,
    #[serde(rename = "cutType")]
    pub cut_type: CutType,
    #[serde(rename = "scoredTabSpeedMultiplier")]
    pub scored_tab_speed_multiplier: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lead {
    pub points: Vec<PathPoint>,
    pub closed: bool,
    #[serde(rename = "baseBB")]
    pub base_bb: BBox,
    pub center: Point,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CutOrder {
    pub mode: CutOrderMode,
    pub groups: Vec<CutOrderGroup>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CutOrderGroup {
    #[serde(rename = "pathGroupID")]
    pub path_group_id: u64,
    /// Only `[null]` has been observed; anything else is rejected.
    pub segments: Vec<()>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    #[serde(rename = "fileID")]
    pub file_id: u64,
    #[serde(rename = "baseBB")]
    pub base_bb: BBox,
    /// Rotated/scaled box centred on the instance origin (instance 0's scale).
    #[serde(rename = "RSTBB")]
    pub rst_bb: BBox,
    pub instances: Vec<Instance>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Instance {
    pub scale: f64,
    /// Degrees; only 0 observed.
    pub rotation: f64,
    /// Inches on the bed: where the RSTBB origin lands.
    pub position: Point,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LengthUnit {
    #[serde(rename = "mm")]
    Mm,
    #[serde(rename = "in")]
    In,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    #[serde(rename = "roughRate")]
    Rough,
    #[serde(rename = "mediumRate")]
    Medium,
    #[serde(rename = "fineRate")]
    Fine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AbrasiveMode {
    #[serde(rename = "speedMode")]
    Speed,
    #[serde(rename = "ecoMode")]
    Eco,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MachineType {
    #[serde(rename = "pro")]
    Pro,
    #[serde(rename = "desktop")]
    Desktop,
}

/// `Outside` is not in any sample yet; it is accepted because WAM's UI offers it
/// and the exporter needs it, and it is listed as unverified in the spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OffsetType {
    Inside,
    Outside,
    Centerline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TabType {
    #[serde(rename = "automatic")]
    Automatic,
    #[serde(rename = "none")]
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CutType {
    #[serde(rename = "standard")]
    Standard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CutOrderMode {
    #[serde(rename = "automatic")]
    Automatic,
}
