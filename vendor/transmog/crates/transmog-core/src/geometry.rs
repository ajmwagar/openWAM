//! Geometry primitives.
//!
//! Everything is `f64` (glam's `DVec*`): a mesh-to-CAD pipeline snaps
//! dimensions to sub-micron tool/stock sizes, so `f32` precision is not
//! enough once we start refitting and quantizing (design §2.5).

use glam::{DVec2, DVec3};

/// A point (or vector) in the 2D sketch plane.
pub type Point2 = DVec2;

/// A point in 3D model space.
pub type Point3 = DVec3;

/// A direction / displacement in 3D model space.
pub type Vec3 = DVec3;

/// An axis-aligned bounding box in model space.
///
/// Constructed via [`Aabb::from_points`]; the invariant `min <= max`
/// componentwise holds for any non-empty input.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    /// Corner with the smallest coordinate on every axis.
    pub min: Point3,
    /// Corner with the largest coordinate on every axis.
    pub max: Point3,
}

impl Aabb {
    /// Builds the tight bounding box of an iterator of points.
    ///
    /// Returns `None` if the iterator is empty.
    pub fn from_points(points: impl IntoIterator<Item = Point3>) -> Option<Self> {
        let mut iter = points.into_iter();
        let first = iter.next()?;
        let mut min = first;
        let mut max = first;
        for p in iter {
            min = min.min(p);
            max = max.max(p);
        }
        Some(Self { min, max })
    }

    /// The box's extent along each axis (`max - min`).
    #[must_use]
    pub fn size(&self) -> Vec3 {
        self.max - self.min
    }

    /// The geometric center of the box.
    #[must_use]
    pub fn center(&self) -> Point3 {
        self.min.midpoint(self.max)
    }

    /// The longest side length — a convenient scale for tolerances.
    #[must_use]
    pub fn max_extent(&self) -> f64 {
        self.size().max_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_points_empty_is_none() {
        assert!(Aabb::from_points(std::iter::empty()).is_none());
    }

    #[test]
    fn bounds_size_and_center() {
        let pts = [
            Point3::new(-1.0, 0.0, 2.0),
            Point3::new(3.0, -4.0, 2.0),
            Point3::new(0.0, 1.0, 10.0),
        ];
        let bb = Aabb::from_points(pts).expect("non-empty");
        assert_eq!(bb.min, Point3::new(-1.0, -4.0, 2.0));
        assert_eq!(bb.max, Point3::new(3.0, 1.0, 10.0));
        assert_eq!(bb.size(), Vec3::new(4.0, 5.0, 8.0));
        assert_eq!(bb.center(), Point3::new(1.0, -1.5, 6.0));
        assert!((bb.max_extent() - 8.0).abs() < f64::EPSILON);
    }
}
