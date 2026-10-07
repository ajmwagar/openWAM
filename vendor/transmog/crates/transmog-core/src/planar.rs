//! Bounded simple-polygon geometry shared by manufacturing clients.
//! No topology repair, snapping, boolean splitting or hidden unit conversion.
use crate::geometry::Point2;
pub const EPS: f64 = 1e-8;
pub const MAX_POINTS: usize = 2048;
#[derive(Debug, thiserror::Error)]
#[error("invalid planar geometry: {0}")]
pub struct Error(pub &'static str);
fn need(ok: bool, why: &'static str) -> Result<(), Error> {
    if ok {
        Ok(())
    } else {
        Err(Error(why))
    }
}
fn cross(a: Point2, b: Point2) -> f64 {
    a.x * b.y - a.y * b.x
}
pub fn area(p: &[Point2]) -> f64 {
    p.iter()
        .zip(p.iter().cycle().skip(1))
        .take(p.len())
        .map(|(a, b)| cross(*a, *b))
        .sum::<f64>()
        / 2.0
}
pub fn point_segment_distance(p: Point2, a: Point2, b: Point2) -> f64 {
    let d = b - a;
    let t = ((p - a).dot(d) / d.length_squared()).clamp(0.0, 1.0);
    (p - a - d * t).length()
}
pub fn intersects(a: Point2, b: Point2, c: Point2, d: Point2) -> bool {
    let x = cross(b - a, c - a);
    let y = cross(b - a, d - a);
    let z = cross(d - c, a - c);
    let w = cross(d - c, b - c);
    if x * y < 0.0 && z * w < 0.0 {
        return true;
    }
    [
        point_segment_distance(a, c, d),
        point_segment_distance(b, c, d),
        point_segment_distance(c, a, b),
        point_segment_distance(d, a, b),
    ]
    .into_iter()
    .any(|v| v <= EPS)
}
pub fn segment_distance(a: Point2, b: Point2, c: Point2, d: Point2) -> f64 {
    if intersects(a, b, c, d) {
        0.0
    } else {
        [
            point_segment_distance(a, c, d),
            point_segment_distance(b, c, d),
            point_segment_distance(c, a, b),
            point_segment_distance(d, a, b),
        ]
        .into_iter()
        .fold(f64::INFINITY, f64::min)
    }
}
pub fn validate(p: &[Point2]) -> Result<(), Error> {
    need((3..=MAX_POINTS).contains(&p.len()), "polygon point count")?;
    need(
        p.iter()
            .all(|p| p.is_finite() && p.abs().max_element() <= 1e6),
        "polygon numeric bounds",
    )?;
    need(area(p).abs() > EPS, "polygon area is zero")?;
    for i in 0..p.len() {
        let next = (i + 1) % p.len();
        need((p[next] - p[i]).length() > EPS, "degenerate polygon edge")?;
        let prev = (i + p.len() - 1) % p.len();
        let a = (p[i] - p[prev]).normalize();
        let b = (p[next] - p[i]).normalize();
        need(a.dot(b) > -1.0 + EPS, "polygon doubles back")?;
        for j in i + 1..p.len() {
            if j == next || (j + 1) % p.len() == i {
                continue;
            }
            need(
                !intersects(p[i], p[next], p[j], p[(j + 1) % p.len()]),
                "polygon self intersects or touches",
            )?;
        }
    }
    Ok(())
}
pub fn contains(p: &[Point2], q: Point2) -> bool {
    let mut inside = false;
    for i in 0..p.len() {
        let a = p[i];
        let b = p[(i + 1) % p.len()];
        if point_segment_distance(q, a, b) <= EPS {
            return true;
        }
        if (a.y > q.y) != (b.y > q.y) && q.x < (b.x - a.x) * (q.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
    }
    inside
}
pub fn separate(a: &[Point2], b: &[Point2]) -> Result<(), Error> {
    for i in 0..a.len() {
        for j in 0..b.len() {
            need(
                !intersects(a[i], a[(i + 1) % a.len()], b[j], b[(j + 1) % b.len()]),
                "contours cross or touch",
            )?;
        }
    }
    Ok(())
}
/// Positive distance expands independently of winding; negative distance insets.
/// Miter intersections are bounded and results revalidated. Changes requiring
/// topology splitting or edge removal fail rather than dropping material.
pub fn offset(p: &[Point2], distance: f64) -> Result<Vec<Point2>, Error> {
    validate(p)?;
    need(
        distance.is_finite() && distance.abs() <= 1000.0,
        "offset bound",
    )?;
    if distance == 0.0 {
        return Ok(p.to_vec());
    }
    let winding = area(p).signum();
    let mut out = Vec::new();
    for i in 0..p.len() {
        let before = p[(i + p.len() - 1) % p.len()];
        let at = p[i];
        let after = p[(i + 1) % p.len()];
        let u = (at - before).normalize();
        let v = (after - at).normalize();
        let normal = |d: Point2| Point2::new(d.y, -d.x) * winding;
        let a = at + normal(u) * distance;
        let b = at + normal(v) * distance;
        let determinant = cross(u, v);
        let q = if determinant.abs() < EPS {
            need(u.dot(v) > 0.0, "offset cusp")?;
            a
        } else {
            a + u * (cross(b - a, v) / determinant)
        };
        need(
            (q - at).length() <= 4.0 * distance.abs() + EPS,
            "offset miter exceeds limit",
        )?;
        out.push(q);
    }
    validate(&out)?;
    need(area(p) * area(&out) > 0.0, "offset reversed polygon")?;
    for i in 0..p.len() {
        let next = (i + 1) % p.len();
        need(
            (out[next] - out[i]).dot(p[next] - p[i]) > EPS,
            "offset collapsed or reversed edge",
        )?;
    }
    if distance < 0.0 {
        need(out.iter().all(|q| contains(p, *q)), "inset escaped source")?;
    } else {
        need(
            p.iter().all(|q| contains(&out, *q)),
            "expansion lost source",
        )?;
    }
    Ok(out)
}
pub fn length(p: &[Point2]) -> f64 {
    p.iter()
        .zip(p.iter().cycle().skip(1))
        .take(p.len())
        .map(|(a, b)| (*b - *a).length())
        .sum()
}
fn at_distance(p: &[Point2], distance: f64) -> Point2 {
    let mut remaining = distance;
    for i in 0..p.len() {
        let a = p[i];
        let b = p[(i + 1) % p.len()];
        let len = (b - a).length();
        if remaining <= len {
            return a + (b - a) * (remaining / len);
        }
        remaining -= len;
    }
    p[0]
}
/// Inclusive endpoints plus every intervening original vertex; never chord
/// across a polygon corner when splitting leads or holding bridges.
pub fn slice(p: &[Point2], start: f64, end: f64) -> Result<Vec<Point2>, Error> {
    validate(p)?;
    let total = length(p);
    need(
        start.is_finite() && end.is_finite() && start >= 0.0 && end > start && end <= total + EPS,
        "path interval",
    )?;
    let mut out = vec![at_distance(p, start)];
    let mut distance = 0.0;
    for i in 0..p.len() {
        distance += (p[(i + 1) % p.len()] - p[i]).length();
        if distance > start + EPS && distance < end - EPS {
            out.push(p[(i + 1) % p.len()]);
        }
    }
    let finish = at_distance(p, end.min(total));
    if (finish - *out.last().unwrap()).length() > EPS {
        out.push(finish);
    }
    need(out.len() >= 2, "collapsed path interval")?;
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn square() -> Vec<Point2> {
        vec![
            Point2::new(0.0, 0.0),
            Point2::new(10.0, 0.0),
            Point2::new(10.0, 10.0),
            Point2::new(0.0, 10.0),
        ]
    }
    #[test]
    fn winding_independent_offset_and_collapse() {
        let p = square();
        for polygon in [p.clone(), p.into_iter().rev().collect()] {
            let out = offset(&polygon, 0.5).unwrap();
            assert!((area(&out).abs() - 121.0).abs() < EPS);
            let inside = offset(&polygon, -0.5).unwrap();
            assert!((area(&inside).abs() - 81.0).abs() < EPS);
            assert!(offset(&polygon, -6.0).is_err());
        }
    }
    #[test]
    fn rejects_crossings_touching_and_unsafe_miters() {
        let p = square();
        let bad = vec![p[0], p[2], p[1], p[3]];
        assert!(validate(&bad).is_err());
        assert!(separate(&p, &p).is_err());
        let sharp = [
            Point2::new(0.0, 0.0),
            Point2::new(100.0, 0.0),
            Point2::new(0.0, 1.0),
        ];
        assert!(offset(&sharp, 0.5).is_err());
        assert!(slice(&[], 0.0, EPS / 2.0).is_err());
        assert!(contains(&p, Point2::new(5.0, 5.0)));
        assert!(!contains(&p, Point2::new(15.0, 5.0)));
        let cut = slice(&p, 5.0, 15.0).unwrap();
        assert_eq!(
            cut,
            vec![
                Point2::new(5.0, 0.0),
                Point2::new(10.0, 0.0),
                Point2::new(10.0, 5.0)
            ]
        );
    }

    #[test]
    fn concave_offsets_preserve_shape_when_topology_is_unchanged() {
        let p = [
            Point2::new(0.0, 0.0),
            Point2::new(10.0, 0.0),
            Point2::new(10.0, 4.0),
            Point2::new(4.0, 4.0),
            Point2::new(4.0, 10.0),
            Point2::new(0.0, 10.0),
        ];
        let out = offset(&p, 0.5).unwrap();
        assert!((area(&out) - 85.0).abs() < EPS);
        let inset = offset(&p, -0.5).unwrap();
        assert!((area(&inset) - 45.0).abs() < EPS);
        assert!(offset(&p, -2.1).is_err());
    }
}
