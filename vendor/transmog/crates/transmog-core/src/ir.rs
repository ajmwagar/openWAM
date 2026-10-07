//! The editable `FeaturePlan` IR (design §3).
//!
//! This is the source of truth for a reconstructed part: a small, ordered list
//! of parametric features that a geometry kernel replays to produce a solid,
//! and that the hand-dial GUI edits directly. It is deliberately a fixed, small
//! vocabulary — not a general scripting language — so recognition stays a
//! greedy match against known targets and every GUI editor is a known widget.
//!
//! Serialized form is RON ([`FeaturePlan::to_ron`] / [`FeaturePlan::from_ron`]).

use glam::DQuat;
use serde::{Deserialize, Serialize};

use crate::geometry::{Point2, Point3, Vec3};

/// Stable identifier for a feature within a [`FeaturePlan`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FeatureId(pub u32);

/// A reference plane for sketches, mirrors, etc.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum PlaneRef {
    /// World XY plane (normal +Z).
    Xy,
    /// World YZ plane (normal +X).
    Yz,
    /// World XZ plane (normal +Y).
    Xz,
    /// An arbitrary plane through `origin` with the given `normal`.
    Custom {
        /// A point on the plane.
        origin: Point3,
        /// The plane normal (need not be unit length).
        normal: Vec3,
    },
}

/// A reference axis for revolves and circular patterns.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum AxisRef {
    /// World X axis.
    X,
    /// World Y axis.
    Y,
    /// World Z axis.
    Z,
    /// An arbitrary axis through `origin` along `direction`.
    Custom {
        /// A point on the axis.
        origin: Point3,
        /// The axis direction (need not be unit length).
        direction: Vec3,
    },
}

/// A reference to a topological edge of a previously-created feature's solid.
///
/// Edges are identified by the ordinal the kernel assigns when it enumerates
/// the feature's edges — a pragmatic handle until a stable naming scheme lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeRef {
    /// The feature whose solid owns the edge.
    pub feature: FeatureId,
    /// Index of the edge in that feature's enumerated edge list.
    pub edge_index: u32,
}

/// A boolean operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoolOp {
    /// Fuse tool into target.
    Union,
    /// Subtract tool from target.
    Difference,
    /// Keep the overlap of tool and target.
    Intersection,
}

/// A rigid+scale transform for an explicit instance.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    /// Translation.
    pub translation: Vec3,
    /// Rotation quaternion.
    pub rotation: DQuat,
    /// Per-axis scale.
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: DQuat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}

/// A single curve of a 2D sketch profile, in sketch-plane coordinates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SketchSegment {
    /// Straight segment.
    Line {
        /// Start point.
        start: Point2,
        /// End point.
        end: Point2,
    },
    /// Circular arc from `start` to `end` about `center`.
    Arc {
        /// Arc start.
        start: Point2,
        /// Arc end.
        end: Point2,
        /// Circle center.
        center: Point2,
    },
    /// B-spline through its control points (`GeomAPI_PointsToBSpline`).
    Spline {
        /// Ordered control points.
        control_points: Vec<Point2>,
    },
}

/// A parametric feature. Fixed, small vocabulary by design (§3).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Feature {
    /// A 2D profile on a plane.
    Sketch {
        /// The plane the profile lives on.
        plane: PlaneRef,
        /// Ordered profile segments.
        profile: Vec<SketchSegment>,
    },
    /// Extrude a sketch to a solid.
    Extrude {
        /// The sketch to extrude.
        sketch: FeatureId,
        /// Extrusion distance in mm.
        distance_mm: f64,
        /// Extrude symmetrically about the sketch plane.
        symmetric: bool,
    },
    /// Revolve a sketch about an axis.
    Revolve {
        /// The sketch to revolve.
        sketch: FeatureId,
        /// The revolve axis.
        axis: AxisRef,
        /// Sweep angle in degrees.
        angle_deg: f64,
    },
    /// Loft through ordered profiles.
    Loft {
        /// Profiles to loft through, in order.
        profiles: Vec<FeatureId>,
    },
    /// Sweep a profile along a path.
    Sweep {
        /// The swept profile.
        profile: FeatureId,
        /// The sweep path.
        path: FeatureId,
    },
    /// Round edges.
    Fillet {
        /// Edges to round.
        edges: Vec<EdgeRef>,
        /// Fillet radius in mm.
        radius_mm: f64,
    },
    /// Bevel edges.
    Chamfer {
        /// Edges to bevel.
        edges: Vec<EdgeRef>,
        /// Chamfer setback in mm.
        distance_mm: f64,
    },
    /// Boolean combine two features.
    Boolean {
        /// The operation.
        op: BoolOp,
        /// The target solid.
        target: FeatureId,
        /// The tool solid.
        tool: FeatureId,
    },
    /// Mirror a feature across a plane.
    Mirror {
        /// The feature to mirror.
        source: FeatureId,
        /// The mirror plane.
        plane: PlaneRef,
    },
    /// Repeat a feature along a direction.
    LinearPattern {
        /// The feature to repeat.
        source: FeatureId,
        /// Pattern direction.
        direction: Vec3,
        /// Number of copies (including the original).
        count: u32,
        /// Spacing between copies in mm.
        spacing_mm: f64,
    },
    /// Repeat a feature about an axis.
    CircularPattern {
        /// The feature to repeat.
        source: FeatureId,
        /// The pattern axis.
        axis: AxisRef,
        /// Number of copies (including the original).
        count: u32,
        /// Total angular span in degrees.
        angle_deg: f64,
    },
    /// An explicit single copy with its own transform.
    Instance {
        /// The feature to copy.
        source: FeatureId,
        /// The copy's transform.
        transform: Transform,
    },
}

impl Feature {
    /// The feature IDs this feature depends on (must precede it in the plan).
    #[must_use]
    pub fn referenced_features(&self) -> Vec<FeatureId> {
        match self {
            Feature::Sketch { .. } => Vec::new(),
            Feature::Extrude { sketch, .. } => vec![*sketch],
            Feature::Revolve { sketch, .. } => vec![*sketch],
            Feature::Loft { profiles } => profiles.clone(),
            Feature::Sweep { profile, path } => vec![*profile, *path],
            Feature::Fillet { edges, .. } | Feature::Chamfer { edges, .. } => {
                edges.iter().map(|e| e.feature).collect()
            }
            Feature::Boolean { target, tool, .. } => vec![*target, *tool],
            Feature::Mirror { source, .. }
            | Feature::LinearPattern { source, .. }
            | Feature::CircularPattern { source, .. }
            | Feature::Instance { source, .. } => vec![*source],
        }
    }

    /// A short kind name, handy for GUI labels and diagnostics.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Feature::Sketch { .. } => "Sketch",
            Feature::Extrude { .. } => "Extrude",
            Feature::Revolve { .. } => "Revolve",
            Feature::Loft { .. } => "Loft",
            Feature::Sweep { .. } => "Sweep",
            Feature::Fillet { .. } => "Fillet",
            Feature::Chamfer { .. } => "Chamfer",
            Feature::Boolean { .. } => "Boolean",
            Feature::Mirror { .. } => "Mirror",
            Feature::LinearPattern { .. } => "LinearPattern",
            Feature::CircularPattern { .. } => "CircularPattern",
            Feature::Instance { .. } => "Instance",
        }
    }
}

/// How faithfully a reconstruction is parameterized (hybrid-surfacing spec).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ParametricTier {
    /// A real feature tree (sketch/extrude/revolve/boolean) — hand-dialable.
    #[default]
    Full,
    /// Typed primitive + generic B-spline patches, sewn into one shell, with no
    /// feature semantics. Higher fidelity than `Freeform` where fitting was
    /// confident, but not hand-dialable.
    HybridWrap,
    /// The §6 fallback: everything wrapped generically, no typing.
    Freeform,
}

/// An ordered, parametric reconstruction plan (design §3).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FeaturePlan {
    /// Features in execution order.
    pub features: Vec<(FeatureId, Feature)>,
    /// Content hash of the source mesh this plan was fitted from.
    pub source_mesh_hash: String,
    /// How faithfully this plan is parameterized.
    pub tier: ParametricTier,
}

/// Errors from constructing, validating, or (de)serializing a plan.
#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    /// A feature id appears more than once.
    #[error("duplicate feature id {0:?}")]
    DuplicateId(FeatureId),
    /// A feature references an id that does not exist earlier in the plan.
    #[error("feature {referrer:?} references {missing:?}, which is not defined before it")]
    UnresolvedReference {
        /// The feature holding the dangling reference.
        referrer: FeatureId,
        /// The id it referenced.
        missing: FeatureId,
    },
    /// RON serialization failed.
    #[error("RON serialization failed: {0}")]
    Serialize(#[from] ron::Error),
    /// RON deserialization failed.
    #[error("RON parse failed: {0}")]
    Deserialize(#[from] ron::error::SpannedError),
}

impl FeaturePlan {
    /// Creates an empty [`ParametricTier::Full`] plan for the given mesh hash.
    #[must_use]
    pub fn new(source_mesh_hash: impl Into<String>) -> Self {
        Self {
            features: Vec::new(),
            source_mesh_hash: source_mesh_hash.into(),
            tier: ParametricTier::Full,
        }
    }

    /// An empty [`ParametricTier::Freeform`] plan (the §6 fallback).
    #[must_use]
    pub fn freeform(source_mesh_hash: impl Into<String>) -> Self {
        Self {
            features: Vec::new(),
            source_mesh_hash: source_mesh_hash.into(),
            tier: ParametricTier::Freeform,
        }
    }

    /// Whether this plan is a full, evaluable feature tree.
    #[must_use]
    pub fn parametric(&self) -> bool {
        matches!(self.tier, ParametricTier::Full)
    }

    /// Looks up a feature by id.
    #[must_use]
    pub fn get(&self, id: FeatureId) -> Option<&Feature> {
        self.features
            .iter()
            .find_map(|(fid, f)| (*fid == id).then_some(f))
    }

    /// Checks id uniqueness and that every reference resolves to an earlier
    /// feature (execution order is a valid topological order).
    ///
    /// # Errors
    /// Returns the first [`PlanError`] found.
    pub fn validate(&self) -> Result<(), PlanError> {
        let mut seen = std::collections::HashSet::new();
        for (id, feature) in &self.features {
            for r in feature.referenced_features() {
                if !seen.contains(&r) {
                    return Err(PlanError::UnresolvedReference {
                        referrer: *id,
                        missing: r,
                    });
                }
            }
            if !seen.insert(*id) {
                return Err(PlanError::DuplicateId(*id));
            }
        }
        Ok(())
    }

    /// Serializes to a pretty RON string.
    ///
    /// # Errors
    /// [`PlanError::Serialize`] on failure.
    pub fn to_ron(&self) -> Result<String, PlanError> {
        let config = ron::ser::PrettyConfig::new().struct_names(true);
        Ok(ron::ser::to_string_pretty(self, config)?)
    }

    /// Parses a plan from a RON string (does not validate references).
    ///
    /// # Errors
    /// [`PlanError::Deserialize`] on failure.
    pub fn from_ron(text: &str) -> Result<Self, PlanError> {
        Ok(ron::from_str(text)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cube = square sketch on XY, extruded 10 mm.
    fn cube_plan() -> FeaturePlan {
        let sketch = FeatureId(0);
        let extrude = FeatureId(1);
        let half = 5.0;
        FeaturePlan {
            features: vec![
                (
                    sketch,
                    Feature::Sketch {
                        plane: PlaneRef::Xy,
                        profile: vec![
                            SketchSegment::Line {
                                start: Point2::new(-half, -half),
                                end: Point2::new(half, -half),
                            },
                            SketchSegment::Line {
                                start: Point2::new(half, -half),
                                end: Point2::new(half, half),
                            },
                            SketchSegment::Line {
                                start: Point2::new(half, half),
                                end: Point2::new(-half, half),
                            },
                            SketchSegment::Line {
                                start: Point2::new(-half, half),
                                end: Point2::new(-half, -half),
                            },
                        ],
                    },
                ),
                (
                    extrude,
                    Feature::Extrude {
                        sketch,
                        distance_mm: 10.0,
                        symmetric: false,
                    },
                ),
            ],
            source_mesh_hash: "deadbeefdeadbeef".to_owned(),
            tier: ParametricTier::Full,
        }
    }

    #[test]
    fn ron_round_trips() {
        let plan = cube_plan();
        let text = plan.to_ron().expect("serialize");
        let back = FeaturePlan::from_ron(&text).expect("parse");
        assert_eq!(plan, back);
    }

    #[test]
    fn validate_accepts_well_formed_plan() {
        assert!(cube_plan().validate().is_ok());
    }

    #[test]
    fn validate_rejects_forward_reference() {
        // Extrude references a sketch defined *after* it.
        let plan = FeaturePlan {
            features: vec![
                (
                    FeatureId(1),
                    Feature::Extrude {
                        sketch: FeatureId(0),
                        distance_mm: 10.0,
                        symmetric: false,
                    },
                ),
                (
                    FeatureId(0),
                    Feature::Sketch {
                        plane: PlaneRef::Xy,
                        profile: Vec::new(),
                    },
                ),
            ],
            source_mesh_hash: String::new(),
            tier: ParametricTier::Full,
        };
        assert!(matches!(
            plan.validate(),
            Err(PlanError::UnresolvedReference { .. })
        ));
    }

    #[test]
    fn validate_rejects_duplicate_ids() {
        let mut plan = cube_plan();
        plan.features[1].0 = FeatureId(0); // collide with the sketch id
        assert!(matches!(plan.validate(), Err(PlanError::DuplicateId(_))));
    }

    #[test]
    fn get_finds_features() {
        let plan = cube_plan();
        assert_eq!(plan.get(FeatureId(0)).map(Feature::kind), Some("Sketch"));
        assert_eq!(plan.get(FeatureId(1)).map(Feature::kind), Some("Extrude"));
        assert!(plan.get(FeatureId(9)).is_none());
    }
}
