//! Triangle-mesh container passed between pipeline stages.

use crate::geometry::{Aabb, Point3, Vec3};

/// A triangle mesh in model space.
///
/// This is the internal representation produced by ingest (design §2.1) and
/// consumed by the rasterizer (§2.3) and segmentation (§2.4). It intentionally
/// stores only what the pipeline needs: positions, triangle indices, optional
/// per-vertex normals, and the unit scale recovered from glb metadata so
/// downstream fitting can reason in millimetres.
#[derive(Clone, Debug)]
pub struct MeshData {
    /// Vertex positions, in source units (see [`MeshData::unit_scale_mm`]).
    pub positions: Vec<Point3>,
    /// Triangles as triples of indices into `positions`.
    pub triangles: Vec<[u32; 3]>,
    /// Optional per-vertex normals; empty if the source provided none.
    pub normals: Vec<Vec3>,
    /// Millimetres per source unit, from glb `asset`/node scale metadata.
    ///
    /// `1.0` means positions are already in millimetres.
    pub unit_scale_mm: f64,
}

impl Default for MeshData {
    fn default() -> Self {
        Self {
            positions: Vec::new(),
            triangles: Vec::new(),
            normals: Vec::new(),
            unit_scale_mm: 1.0,
        }
    }
}

impl MeshData {
    /// Number of vertices.
    #[must_use]
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// Number of triangles.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }

    /// Whether the mesh has any geometry.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.triangles.is_empty()
    }

    /// Axis-aligned bounding box of the vertices, or `None` if empty.
    #[must_use]
    pub fn aabb(&self) -> Option<Aabb> {
        Aabb::from_points(self.positions.iter().copied())
    }

    /// Arithmetic mean of the vertex positions, or `None` if empty.
    #[must_use]
    pub fn centroid(&self) -> Option<Point3> {
        if self.positions.is_empty() {
            return None;
        }
        let sum: Point3 = self.positions.iter().copied().sum();
        Some(sum / self.positions.len() as f64)
    }

    /// Returns the three vertex positions of triangle `i`, if in range.
    #[must_use]
    pub fn triangle_positions(&self, i: usize) -> Option<[Point3; 3]> {
        let [a, b, c] = *self.triangles.get(i)?;
        Some([
            self.positions[a as usize],
            self.positions[b as usize],
            self.positions[c as usize],
        ])
    }

    /// Absolute volume enclosed by the mesh (assumes it is closed).
    ///
    /// Sum of signed tetrahedron volumes from the origin; the sign depends on
    /// winding, so the magnitude is returned.
    #[must_use]
    pub fn volume(&self) -> f64 {
        let mut v6 = 0.0;
        for t in &self.triangles {
            let a = self.positions[t[0] as usize];
            let b = self.positions[t[1] as usize];
            let c = self.positions[t[2] as usize];
            v6 += a.dot(b.cross(c));
        }
        (v6 / 6.0).abs()
    }

    /// A stable content hash of the geometry (positions + triangles), as a
    /// 16-char hex string.
    ///
    /// Used as `FeaturePlan::source_mesh_hash` to tie a plan to the mesh it was
    /// fitted from. This is FNV-1a over the raw bit patterns, so it is
    /// deterministic across runs and platforms — unlike `std`'s `DefaultHasher`
    /// — but it is not order-invariant: reordering vertices changes the hash.
    #[must_use]
    pub fn content_hash(&self) -> String {
        const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut hash = OFFSET;
        let mut eat = |bytes: &[u8]| {
            for &b in bytes {
                hash ^= u64::from(b);
                hash = hash.wrapping_mul(PRIME);
            }
        };
        for p in &self.positions {
            eat(&p.x.to_bits().to_le_bytes());
            eat(&p.y.to_bits().to_le_bytes());
            eat(&p.z.to_bits().to_le_bytes());
        }
        for t in &self.triangles {
            for idx in t {
                eat(&idx.to_le_bytes());
            }
        }
        format!("{hash:016x}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_tri() -> MeshData {
        MeshData {
            positions: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(2.0, 0.0, 0.0),
                Point3::new(0.0, 2.0, 0.0),
            ],
            triangles: vec![[0, 1, 2]],
            normals: Vec::new(),
            unit_scale_mm: 1.0,
        }
    }

    #[test]
    fn default_is_empty_mm_scaled() {
        let m = MeshData::default();
        assert!(m.is_empty());
        assert_eq!(m.vertex_count(), 0);
        assert!(m.aabb().is_none());
        assert!(m.centroid().is_none());
        assert_eq!(m.unit_scale_mm, 1.0);
    }

    #[test]
    fn counts_and_bounds() {
        let m = unit_tri();
        assert_eq!(m.vertex_count(), 3);
        assert_eq!(m.triangle_count(), 1);
        let bb = m.aabb().expect("non-empty");
        assert_eq!(bb.size(), Vec3::new(2.0, 2.0, 0.0));
        let c = m.centroid().expect("non-empty");
        assert!((c - Point3::new(2.0 / 3.0, 2.0 / 3.0, 0.0)).length() < 1e-12);
    }

    #[test]
    fn triangle_positions_bounds_checked() {
        let m = unit_tri();
        assert!(m.triangle_positions(0).is_some());
        assert!(m.triangle_positions(1).is_none());
    }

    #[test]
    fn volume_of_a_unit_box() {
        // Two unit triangles don't enclose volume; a closed box does.
        let two = 2.0;
        let m = MeshData {
            positions: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(two, 0.0, 0.0),
                Point3::new(two, two, 0.0),
                Point3::new(0.0, two, 0.0),
                Point3::new(0.0, 0.0, two),
                Point3::new(two, 0.0, two),
                Point3::new(two, two, two),
                Point3::new(0.0, two, two),
            ],
            triangles: vec![
                [0, 2, 1],
                [0, 3, 2],
                [4, 5, 6],
                [4, 6, 7],
                [0, 1, 5],
                [0, 5, 4],
                [2, 3, 7],
                [2, 7, 6],
                [1, 2, 6],
                [1, 6, 5],
                [3, 0, 4],
                [3, 4, 7],
            ],
            normals: Vec::new(),
            unit_scale_mm: 1.0,
        };
        assert!((m.volume() - 8.0).abs() < 1e-9, "2×2×2 box = 8");
    }

    #[test]
    fn content_hash_is_stable_and_sensitive() {
        let a = unit_tri();
        let b = unit_tri();
        assert_eq!(a.content_hash(), b.content_hash(), "same mesh → same hash");
        assert_eq!(a.content_hash().len(), 16);

        let mut moved = unit_tri();
        moved.positions[0].x += 0.001;
        assert_ne!(
            a.content_hash(),
            moved.content_hash(),
            "moved vertex → different hash"
        );
    }
}
