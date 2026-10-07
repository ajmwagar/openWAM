//! Core types shared across the transmog pipeline.
//!
//! This crate is deliberately dependency-light: it holds the geometry
//! primitives ([`geometry`]) and the triangle-mesh container ([`mesh`]) that
//! every downstream stage (ingest, raster/trace, segmentation, kernel eval)
//! operates on. The editable `FeaturePlan` IR (design §3) will land here too,
//! under a future `ir` module, once Phase 1 defines it.
//!
//! See `mesh-to-step-DESIGN.md` for how these types fit the wider pipeline.

pub mod geometry;
pub mod ir;
pub mod mesh;
pub mod planar;

/// Common imports for pipeline code.
pub mod prelude {
    pub use crate::geometry::{Aabb, Point2, Point3, Vec3};
    pub use crate::ir::{Feature, FeatureId, FeaturePlan, ParametricTier, PlaneRef, SketchSegment};
    pub use crate::mesh::MeshData;
}
