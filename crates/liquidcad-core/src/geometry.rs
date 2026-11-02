//! Exact part geometry, rigid poses, and mass properties.
//!
//! A component is a [`Shape`] in its own local frame plus a [`Pose`] that
//! places it in the world. Lengths are inches. Box and straight-sided prism
//! shapes are polyhedra, so this module can give their exact boundary
//! ([`Polyhedron`]) and exact mass properties. Arcs, holes, revolves, and
//! modifier operations need OpenCascade; [`Shape::needs_occt`] says so
//! instead of approximating them.
//!
//! Faces and edges are named from how the shape was built, not from where
//! they ended up: a box has faces `x-`, `x+`, `y-`, `y+`, `z-`, `z+`; a
//! prism has `bottom`, `top`, and `side.N` (N = profile segment). An edge is
//! named by its two faces, sorted and joined with `|`. Names therefore
//! survive any parameter change that keeps the face, which is what a
//! fillet, a mate, or a sketch plane needs to keep pointing at the right
//! thing.

use serde::{Deserialize, Serialize};

pub type Vec3 = [f64; 3];
pub type Mat3 = [[f64; 3]; 3];

pub fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn scale(a: Vec3, s: f64) -> Vec3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub fn norm(a: Vec3) -> f64 {
    dot(a, a).sqrt()
}
pub fn normalize(a: Vec3) -> Vec3 {
    let n = norm(a);
    if n > 0.0 {
        scale(a, 1.0 / n)
    } else {
        a
    }
}

fn mat_mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut out = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}
fn transpose(a: &Mat3) -> Mat3 {
    let mut out = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = a[j][i];
        }
    }
    out
}
fn outer(a: Vec3, b: Vec3) -> Mat3 {
    let mut out = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = a[i] * b[j];
        }
    }
    out
}
fn mat_add(a: &Mat3, b: &Mat3, s: f64) -> Mat3 {
    let mut out = *a;
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] += s * b[i][j];
        }
    }
    out
}
fn mat_vec(a: &Mat3, v: Vec3) -> Vec3 {
    [dot(a[0], v), dot(a[1], v), dot(a[2], v)]
}

/// Rigid transform: rotate by a unit quaternion `[w, x, y, z]`, then
/// translate by `origin`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pose {
    pub origin: Vec3,
    pub rotation: [f64; 4],
}

impl Default for Pose {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Pose {
    pub const IDENTITY: Self = Self {
        origin: [0.0; 3],
        rotation: [1.0, 0.0, 0.0, 0.0],
    };

    pub fn translation(origin: Vec3) -> Self {
        Self {
            origin,
            ..Self::IDENTITY
        }
    }

    /// Rotation by `angle` radians about `axis` through the local origin.
    pub fn rotation(axis: Vec3, angle: f64) -> Self {
        let a = normalize(axis);
        let (s, c) = (angle * 0.5).sin_cos();
        Self {
            origin: [0.0; 3],
            rotation: [c, a[0] * s, a[1] * s, a[2] * s],
        }
    }

    /// Frame with the given orthonormal axes as its local x, y, z.
    pub fn from_axes(origin: Vec3, x: Vec3, y: Vec3, z: Vec3) -> Self {
        // Rotation matrix with columns x, y, z to a quaternion (Shepperd).
        let m = [[x[0], y[0], z[0]], [x[1], y[1], z[1]], [x[2], y[2], z[2]]];
        let tr = m[0][0] + m[1][1] + m[2][2];
        let q = if tr > 0.0 {
            let s = (tr + 1.0).sqrt() * 2.0;
            [
                0.25 * s,
                (m[2][1] - m[1][2]) / s,
                (m[0][2] - m[2][0]) / s,
                (m[1][0] - m[0][1]) / s,
            ]
        } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
            let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
            [
                (m[2][1] - m[1][2]) / s,
                0.25 * s,
                (m[0][1] + m[1][0]) / s,
                (m[0][2] + m[2][0]) / s,
            ]
        } else if m[1][1] > m[2][2] {
            let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
            [
                (m[0][2] - m[2][0]) / s,
                (m[0][1] + m[1][0]) / s,
                0.25 * s,
                (m[1][2] + m[2][1]) / s,
            ]
        } else {
            let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
            [
                (m[1][0] - m[0][1]) / s,
                (m[0][2] + m[2][0]) / s,
                (m[1][2] + m[2][1]) / s,
                0.25 * s,
            ]
        };
        Self {
            origin,
            rotation: quat_normalize(q),
        }
    }

    /// Rotation by `angle` radians about the line through `point` along `axis`.
    pub fn rotation_about(point: Vec3, axis: Vec3, angle: f64) -> Self {
        Self::translation(point)
            .then(&Self::rotation(axis, angle))
            .then(&Self::translation(scale(point, -1.0)))
    }

    pub fn is_identity(&self) -> bool {
        self.origin == [0.0; 3] && self.rotation == [1.0, 0.0, 0.0, 0.0]
    }

    pub fn matrix(&self) -> Mat3 {
        let [w, x, y, z] = self.rotation;
        [
            [
                1.0 - 2.0 * (y * y + z * z),
                2.0 * (x * y - w * z),
                2.0 * (x * z + w * y),
            ],
            [
                2.0 * (x * y + w * z),
                1.0 - 2.0 * (x * x + z * z),
                2.0 * (y * z - w * x),
            ],
            [
                2.0 * (x * z - w * y),
                2.0 * (y * z + w * x),
                1.0 - 2.0 * (x * x + y * y),
            ],
        ]
    }

    pub fn rotate(&self, v: Vec3) -> Vec3 {
        if self.rotation == [1.0, 0.0, 0.0, 0.0] {
            return v;
        }
        mat_vec(&self.matrix(), v)
    }

    pub fn apply(&self, p: Vec3) -> Vec3 {
        add(self.rotate(p), self.origin)
    }

    /// `self ∘ inner`: apply `inner` first, then `self`.
    pub fn then(&self, inner: &Pose) -> Pose {
        let q = quat_normalize(quat_mul(self.rotation, inner.rotation));
        Pose {
            origin: self.apply(inner.origin),
            rotation: q,
        }
    }

    pub fn inverse(&self) -> Pose {
        let [w, x, y, z] = self.rotation;
        let inv = Pose {
            origin: [0.0; 3],
            rotation: [w, -x, -y, -z],
        };
        Pose {
            origin: scale(inv.rotate(self.origin), -1.0),
            rotation: inv.rotation,
        }
    }

    /// Row-major 3x4 matrix `[R | t]`, the layout OpenCascade's gp_Trsf takes.
    pub fn matrix_3x4(&self) -> [f64; 12] {
        let r = self.matrix();
        let t = self.origin;
        [
            r[0][0], r[0][1], r[0][2], t[0], r[1][0], r[1][1], r[1][2], t[1], r[2][0], r[2][1],
            r[2][2], t[2],
        ]
    }

    pub fn is_finite(&self) -> bool {
        self.origin
            .iter()
            .chain(self.rotation.iter())
            .all(|v| v.is_finite())
    }
}

fn quat_mul(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let [aw, ax, ay, az] = a;
    let [bw, bx, by, bz] = b;
    [
        aw * bw - ax * bx - ay * by - az * bz,
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
    ]
}

fn quat_normalize(q: [f64; 4]) -> [f64; 4] {
    let n = (q.iter().map(|v| v * v).sum::<f64>()).sqrt();
    if n == 0.0 {
        [1.0, 0.0, 0.0, 0.0]
    } else {
        q.map(|v| v / n)
    }
}

/// A closed 2D path. Each vertex starts a segment to the next one; `bulge`
/// is tan(θ/4) of that segment's arc (0 = straight), the DXF convention, so
/// profiles round-trip through LWPOLYLINE without approximation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PathVertex {
    pub x: f64,
    pub y: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub bulge: f64,
}

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

impl PathVertex {
    pub fn line(x: f64, y: f64) -> Self {
        Self { x, y, bulge: 0.0 }
    }
    fn p(&self) -> [f64; 2] {
        [self.x, self.y]
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// Outer boundary. Stored counter-clockwise after [`Profile::normalized`].
    pub outer: Vec<PathVertex>,
    /// Inner boundaries (through-holes). Clockwise after normalization.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<Vec<PathVertex>>,
}

impl Profile {
    pub fn rectangle(w: f64, h: f64) -> Self {
        Self {
            outer: vec![
                PathVertex::line(0.0, 0.0),
                PathVertex::line(w, 0.0),
                PathVertex::line(w, h),
                PathVertex::line(0.0, h),
            ],
            holes: Vec::new(),
        }
    }

    pub fn has_arcs(&self) -> bool {
        self.outer
            .iter()
            .chain(self.holes.iter().flatten())
            .any(|v| v.bulge != 0.0)
    }

    /// Checks closure, size, finiteness, and simplicity. Returns the profile
    /// with the outer loop counter-clockwise and holes clockwise.
    pub fn normalized(&self) -> Result<Profile, String> {
        const MAX_VERTICES: usize = 500;
        let total = self.outer.len() + self.holes.iter().map(Vec::len).sum::<usize>();
        if total > MAX_VERTICES {
            return Err(format!(
                "profile has {total} vertices; the limit is {MAX_VERTICES}"
            ));
        }
        let mut out = self.clone();
        check_loop(&out.outer, "outer boundary")?;
        if signed_area(&sample_loop(&out.outer)) < 0.0 {
            reverse_loop(&mut out.outer);
        }
        let outer_poly = sample_loop(&out.outer);
        for (i, hole) in out.holes.iter_mut().enumerate() {
            check_loop(hole, &format!("hole {}", i + 1))?;
            if signed_area(&sample_loop(hole)) > 0.0 {
                reverse_loop(hole);
            }
            let poly = sample_loop(hole);
            if poly.iter().any(|p| !point_in_polygon(*p, &outer_poly)) {
                return Err(format!("hole {} is not inside the outer boundary", i + 1));
            }
            if let Some(p) = polylines_cross(&poly, &outer_poly) {
                return Err(format!(
                    "hole {} crosses the outer boundary at ({}, {})",
                    i + 1,
                    crate::parameters::fmt_num(round3(p[0])),
                    crate::parameters::fmt_num(round3(p[1]))
                ));
            }
        }
        for i in 0..out.holes.len() {
            for j in i + 1..out.holes.len() {
                let (a, b) = (sample_loop(&out.holes[i]), sample_loop(&out.holes[j]));
                if polylines_cross(&a, &b).is_some() || point_in_polygon(a[0], &b) {
                    return Err(format!("holes {} and {} overlap", i + 1, j + 1));
                }
            }
        }
        Ok(out)
    }

    /// Area enclosed, holes removed, arcs exact.
    pub fn area(&self) -> f64 {
        loop_area(&self.outer).abs() - self.holes.iter().map(|h| loop_area(h).abs()).sum::<f64>()
    }

    pub fn bounds(&self) -> ([f64; 2], [f64; 2]) {
        let poly = sample_loop(&self.outer);
        let mut lo = [f64::MAX; 2];
        let mut hi = [f64::MIN; 2];
        for p in poly {
            for i in 0..2 {
                lo[i] = lo[i].min(p[i]);
                hi[i] = hi[i].max(p[i]);
            }
        }
        (lo, hi)
    }
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn reverse_loop(path: &mut Vec<PathVertex>) {
    // Segment i runs v[i] -> v[i+1] with bulge b[i]. Reversed, it runs
    // v[i+1] -> v[i] with bulge -b[i].
    let n = path.len();
    let mut out = Vec::with_capacity(n);
    for k in 0..n {
        let i = (n - k) % n;
        let prev = (i + n - 1) % n;
        out.push(PathVertex {
            x: path[i].x,
            y: path[i].y,
            bulge: -path[prev].bulge,
        });
    }
    *path = out;
}

fn check_loop(path: &[PathVertex], what: &str) -> Result<(), String> {
    if path.len() < 2 || (path.len() < 3 && path.iter().all(|v| v.bulge == 0.0)) {
        return Err(format!("{what} needs at least 3 corners"));
    }
    if path
        .iter()
        .any(|v| !v.x.is_finite() || !v.y.is_finite() || !v.bulge.is_finite())
    {
        return Err(format!("{what} has a non-finite coordinate"));
    }
    if path.iter().any(|v| v.bulge.abs() > 1.0 + 1e-9) {
        return Err(format!(
            "{what} has an arc over 180° (|bulge| > 1); split it in two"
        ));
    }
    for i in 0..path.len() {
        let (a, b) = (path[i].p(), path[(i + 1) % path.len()].p());
        if ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt() < 1e-3 {
            return Err(format!(
                "{what} has a zero-length segment at ({}, {})",
                crate::parameters::fmt_num(round3(a[0])),
                crate::parameters::fmt_num(round3(a[1]))
            ));
        }
    }
    let poly = sample_loop(path);
    if let Some(p) = self_intersection(&poly) {
        return Err(format!(
            "{what} crosses itself at ({}, {})",
            crate::parameters::fmt_num(round3(p[0])),
            crate::parameters::fmt_num(round3(p[1]))
        ));
    }
    if signed_area(&poly).abs() < 1e-6 {
        return Err(format!("{what} encloses no area"));
    }
    Ok(())
}

/// Polyline through the loop with each arc sampled (for validation and
/// bounds only; geometry keeps the exact arc).
fn sample_loop(path: &[PathVertex]) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    for i in 0..path.len() {
        let (a, b) = (path[i], path[(i + 1) % path.len()]);
        out.push(a.p());
        if a.bulge != 0.0 {
            let (c, r, start, sweep) = arc_geometry(a.p(), b.p(), a.bulge);
            let steps = 16;
            for k in 1..steps {
                let t = start + sweep * f64::from(k) / f64::from(steps);
                out.push([c[0] + r * t.cos(), c[1] + r * t.sin()]);
            }
        }
    }
    out
}

/// Centre, radius, start angle, and signed sweep of a bulged segment.
pub fn arc_geometry(a: [f64; 2], b: [f64; 2], bulge: f64) -> ([f64; 2], f64, f64, f64) {
    let sweep = 4.0 * bulge.atan();
    let chord = [(b[0] - a[0]), (b[1] - a[1])];
    let len = (chord[0] * chord[0] + chord[1] * chord[1]).sqrt();
    let r = len / (2.0 * (sweep / 2.0).sin().abs());
    let mid = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
    // Distance from chord midpoint to centre, signed toward the left of a->b
    // for counter-clockwise arcs.
    let h = r * (sweep / 2.0).cos() * sweep.signum();
    let left = [-chord[1] / len, chord[0] / len];
    let c = [mid[0] + left[0] * h, mid[1] + left[1] * h];
    let start = (a[1] - c[1]).atan2(a[0] - c[0]);
    (c, r, start, sweep)
}

fn loop_area(path: &[PathVertex]) -> f64 {
    // Polygon area through the vertices plus each arc's circular segment.
    let pts: Vec<[f64; 2]> = path.iter().map(|v| v.p()).collect();
    let mut area = signed_area(&pts);
    for i in 0..path.len() {
        let (a, b) = (path[i], path[(i + 1) % path.len()]);
        if a.bulge != 0.0 {
            let (_, r, _, sweep) = arc_geometry(a.p(), b.p(), a.bulge);
            area += 0.5 * r * r * (sweep - sweep.sin());
        }
    }
    area
}

pub fn signed_area(p: &[[f64; 2]]) -> f64 {
    let mut a = 0.0;
    for i in 0..p.len() {
        let (u, v) = (p[i], p[(i + 1) % p.len()]);
        a += u[0] * v[1] - v[0] * u[1];
    }
    a * 0.5
}

fn segments_cross(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> Option<[f64; 2]> {
    let r = [b[0] - a[0], b[1] - a[1]];
    let s = [d[0] - c[0], d[1] - c[1]];
    let denom = r[0] * s[1] - r[1] * s[0];
    if denom.abs() < 1e-12 {
        return None;
    }
    let q = [c[0] - a[0], c[1] - a[1]];
    let t = (q[0] * s[1] - q[1] * s[0]) / denom;
    let u = (q[0] * r[1] - q[1] * r[0]) / denom;
    let eps = 1e-9;
    if t > eps && t < 1.0 - eps && u > eps && u < 1.0 - eps {
        Some([a[0] + t * r[0], a[1] + t * r[1]])
    } else {
        None
    }
}

fn self_intersection(p: &[[f64; 2]]) -> Option<[f64; 2]> {
    let n = p.len();
    for i in 0..n {
        for j in i + 1..n {
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            if let Some(x) = segments_cross(p[i], p[(i + 1) % n], p[j], p[(j + 1) % n]) {
                return Some(x);
            }
        }
    }
    // Repeated vertices also make a loop non-simple.
    for i in 0..n {
        for j in i + 1..n {
            if (p[i][0] - p[j][0]).abs() < 1e-9 && (p[i][1] - p[j][1]).abs() < 1e-9 {
                return Some(p[i]);
            }
        }
    }
    None
}

fn polylines_cross(a: &[[f64; 2]], b: &[[f64; 2]]) -> Option<[f64; 2]> {
    for i in 0..a.len() {
        for j in 0..b.len() {
            if let Some(x) = segments_cross(a[i], a[(i + 1) % a.len()], b[j], b[(j + 1) % b.len()])
            {
                return Some(x);
            }
        }
    }
    None
}

pub fn point_in_polygon(p: [f64; 2], poly: &[[f64; 2]]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Ear-clipping triangulation of a simple polygon (either winding).
/// Returns index triples in the polygon's own winding order.
pub fn triangulate(p: &[[f64; 2]]) -> Vec<[usize; 3]> {
    let n = p.len();
    if n < 3 {
        return Vec::new();
    }
    let ccw = signed_area(p) > 0.0;
    let mut idx: Vec<usize> = if ccw {
        (0..n).collect()
    } else {
        (0..n).rev().collect()
    };
    let mut out = Vec::with_capacity(n - 2);
    let cross2 = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut guard = 0;
    while idx.len() > 3 && guard < n * n {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for k in 0..m {
            let (ia, ib, ic) = (idx[(k + m - 1) % m], idx[k], idx[(k + 1) % m]);
            let (a, b, c) = (p[ia], p[ib], p[ic]);
            if cross2(a, b, c) <= 1e-12 {
                continue;
            }
            let same = |u: [f64; 2], v: [f64; 2]| {
                (u[0] - v[0]).abs() < 1e-12 && (u[1] - v[1]).abs() < 1e-12
            };
            let blocked = idx.iter().any(|&iv| {
                if iv == ia || iv == ib || iv == ic {
                    return false;
                }
                let q = p[iv];
                if same(q, a) || same(q, b) || same(q, c) {
                    return false;
                }
                cross2(a, b, q) >= -1e-12 && cross2(b, c, q) >= -1e-12 && cross2(c, a, q) >= -1e-12
            });
            if blocked {
                continue;
            }
            out.push([ia, ib, ic]);
            idx.remove(k);
            clipped = true;
            break;
        }
        if !clipped {
            break;
        }
    }
    if idx.len() == 3 {
        out.push([idx[0], idx[1], idx[2]]);
    }
    if !ccw {
        for t in &mut out {
            t.swap(1, 2);
        }
    }
    out
}

/// Triangulates a polygon with holes by bridging each hole to the outer
/// boundary (rightmost hole first), then ear clipping the result. Returns
/// triangles as indices into `outer` followed by each hole's points.
pub fn triangulate_with_holes(outer: &[[f64; 2]], holes: &[Vec<[f64; 2]>]) -> Vec<[usize; 3]> {
    if holes.is_empty() {
        return triangulate(outer);
    }
    // Work counter-clockwise outside, clockwise holes.
    let mut ring: Vec<usize> = (0..outer.len()).collect();
    let mut pts: Vec<[f64; 2]> = outer.to_vec();
    if signed_area(outer) < 0.0 {
        ring.reverse();
    }
    let mut order: Vec<(usize, usize)> = Vec::new(); // (hole, offset)
    let mut offset = outer.len();
    for (h, hole) in holes.iter().enumerate() {
        order.push((h, offset));
        offset += hole.len();
        pts.extend_from_slice(hole);
    }
    order.sort_by(|a, b| {
        let max = |(h, _): &(usize, usize)| holes[*h].iter().map(|p| p[0]).fold(f64::MIN, f64::max);
        max(b).total_cmp(&max(a))
    });
    for (h, off) in order {
        let hole = &holes[h];
        let mut hidx: Vec<usize> = (0..hole.len()).map(|i| off + i).collect();
        if signed_area(hole) > 0.0 {
            hidx.reverse();
        }
        let (mpos, &m) = hidx
            .iter()
            .enumerate()
            .max_by(|a, b| pts[*a.1][0].total_cmp(&pts[*b.1][0]))
            .expect("hole has points");
        let mp = pts[m];
        // Nearest ring vertex whose bridge crosses no edge.
        let edges: Vec<(usize, usize)> = (0..ring.len())
            .map(|i| (ring[i], ring[(i + 1) % ring.len()]))
            .chain((0..hidx.len()).map(|i| (hidx[i], hidx[(i + 1) % hidx.len()])))
            .collect();
        let mut best: Option<(f64, usize)> = None;
        for (ri, &r) in ring.iter().enumerate() {
            let rp = pts[r];
            let d = (rp[0] - mp[0]).powi(2) + (rp[1] - mp[1]).powi(2);
            if best.is_some_and(|(bd, _)| d >= bd) {
                continue;
            }
            let clear = edges.iter().all(|&(a, b)| {
                a == r
                    || b == r
                    || a == m
                    || b == m
                    || segments_cross(mp, rp, pts[a], pts[b]).is_none()
            });
            if clear {
                best = Some((d, ri));
            }
        }
        let Some((_, ri)) = best else {
            continue;
        };
        let r = ring[ri];
        let mut spliced = Vec::with_capacity(ring.len() + hidx.len() + 2);
        spliced.extend_from_slice(&ring[..=ri]);
        for k in 0..=hidx.len() {
            spliced.push(hidx[(mpos + k) % hidx.len()]);
        }
        spliced.push(r);
        spliced.extend_from_slice(&ring[ri + 1..]);
        ring = spliced;
    }
    let poly: Vec<[f64; 2]> = ring.iter().map(|i| pts[*i]).collect();
    triangulate(&poly)
        .into_iter()
        .map(|t| [ring[t[0]], ring[t[1]], ring[t[2]]])
        .collect()
}

/// Modifier operations. All need OpenCascade.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum ShapeOp {
    Fillet {
        edges: Vec<String>,
        radius: f64,
    },
    Chamfer {
        edges: Vec<String>,
        distance: f64,
    },
    /// Round hole drilled into planar face `face` at local point `at` (on
    /// that face), along the face's inward normal. `depth` 0 = through all.
    Hole {
        face: String,
        at: Vec3,
        diameter: f64,
        depth: f64,
    },
    /// Subtract another shape placed by `pose` in this shape's local frame.
    Cut {
        tool: Box<Shape>,
        pose: Pose,
    },
}

/// Exact geometry in the component's local frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Shape {
    /// Axis-aligned box from the local origin to `size`.
    Box { size: Vec3 },
    /// `profile` in the local XY plane, extruded along +Z by `depth`.
    Prism { profile: Profile, depth: f64 },
    /// `profile` in the local XZ plane (x = radius, y = height) revolved
    /// about local Z by `angle_deg`.
    Revolve { profile: Profile, angle_deg: f64 },
    /// `base` with modifier operations applied in order.
    Modified { base: Box<Shape>, ops: Vec<ShapeOp> },
}

impl Shape {
    pub fn cuboid(size: Vec3) -> Self {
        Self::Box { size }
    }

    /// Why this shape can only be built by OpenCascade, if it can.
    pub fn needs_occt(&self) -> Option<&'static str> {
        match self {
            Self::Box { .. } => None,
            Self::Prism { profile, .. } if profile.has_arcs() => Some("arcs in a profile"),
            Self::Prism { .. } => None,
            Self::Revolve { .. } => Some("revolve"),
            Self::Modified { ops, .. } => ops.first().map(|op| match op {
                ShapeOp::Fillet { .. } => "fillet",
                ShapeOp::Chamfer { .. } => "chamfer",
                ShapeOp::Hole { .. } => "hole",
                ShapeOp::Cut { .. } => "cut",
            }),
        }
    }

    /// Validates dimensions and profiles.
    pub fn validate(&self) -> Result<(), String> {
        let positive = |v: f64, what: &str| {
            if v.is_finite() && v > 0.0 {
                Ok(())
            } else {
                Err(format!("{what} must be a positive number"))
            }
        };
        match self {
            Self::Box { size } => size.iter().try_for_each(|s| positive(*s, "box size")),
            Self::Prism { profile, depth } => {
                positive(*depth, "extrude depth")?;
                profile.normalized().map(|_| ())
            }
            Self::Revolve { profile, angle_deg } => {
                if !(angle_deg.is_finite() && *angle_deg > 0.0 && *angle_deg <= 360.0) {
                    return Err("revolve angle must be in (0, 360] degrees".into());
                }
                let p = profile.normalized()?;
                let (lo, _) = p.bounds();
                if lo[0] < -1e-9 {
                    return Err("revolve profile must stay on the +X side of the axis".into());
                }
                Ok(())
            }
            Self::Modified { base, ops } => {
                base.validate()?;
                for op in ops {
                    match op {
                        ShapeOp::Fillet { edges, radius } => {
                            positive(*radius, "fillet radius")?;
                            if edges.is_empty() {
                                return Err("fillet needs at least one edge".into());
                            }
                        }
                        ShapeOp::Chamfer { edges, distance } => {
                            positive(*distance, "chamfer distance")?;
                            if edges.is_empty() {
                                return Err("chamfer needs at least one edge".into());
                            }
                        }
                        ShapeOp::Hole {
                            diameter,
                            depth,
                            at,
                            ..
                        } => {
                            positive(*diameter, "hole diameter")?;
                            if !(depth.is_finite()
                                && *depth >= 0.0
                                && at.iter().all(|v| v.is_finite()))
                            {
                                return Err("hole position and depth must be finite".into());
                            }
                        }
                        ShapeOp::Cut { tool, pose } => {
                            tool.validate()?;
                            if !pose.is_finite() {
                                return Err("cut tool pose is not finite".into());
                            }
                        }
                    }
                }
                Ok(())
            }
        }
    }

    /// Exact boundary for polyhedral shapes; `None` when OCCT is required.
    pub fn polyhedron(&self) -> Option<Polyhedron> {
        match self {
            Self::Box { size } => Some(Polyhedron::cuboid(*size)),
            Self::Prism { profile, depth } if self.needs_occt().is_none() => {
                let p = profile.normalized().ok()?;
                Some(Polyhedron::prism(&p, *depth))
            }
            _ => None,
        }
    }

    /// Local bounding box. Exact for polyhedra and unmodified revolves; for
    /// modified shapes it is the base's box (modifiers here only remove).
    pub fn local_bounds(&self) -> (Vec3, Vec3) {
        match self {
            Self::Box { size } => ([0.0; 3], *size),
            Self::Prism { profile, depth } => {
                let (lo, hi) = profile.bounds();
                ([lo[0], lo[1], 0.0], [hi[0], hi[1], *depth])
            }
            Self::Revolve { profile, .. } => {
                let (lo, hi) = profile.bounds();
                let r = hi[0].max(lo[0].abs());
                ([-r, -r, lo[1]], [r, r, hi[1]])
            }
            Self::Modified { base, .. } => base.local_bounds(),
        }
    }

    /// Unit-density mass integrals when computable without OCCT.
    pub fn volume_integrals(&self) -> Option<MassIntegrals> {
        self.polyhedron().map(|p| p.integrals())
    }
}

/// Computes exact mass integrals for shapes. The default only handles
/// polyhedra; the kernel crate supplies one backed by OpenCascade.
pub trait ShapeOracle {
    /// Unit-density integrals in the shape's local frame.
    fn integrals(&self, shape: &Shape) -> Result<MassIntegrals, String>;
    /// Whether shapes that need OpenCascade can be built at all.
    fn native(&self) -> bool;
    /// A planar face by construction name, local frame.
    fn face(&self, shape: &Shape, name: &str) -> Result<FaceFrame, String> {
        let poly = shape
            .polyhedron()
            .ok_or_else(|| "faces of this shape need OpenCascade".to_string())?;
        poly.face_frame(name)
            .ok_or_else(|| format!("face {name} does not exist on this part"))
    }
    /// A straight edge by construction name, local frame endpoints.
    fn edge(&self, shape: &Shape, name: &str) -> Result<(Vec3, Vec3), String> {
        let poly = shape
            .polyhedron()
            .ok_or_else(|| "edges of this shape need OpenCascade".to_string())?;
        poly.edge_list()
            .into_iter()
            .find(|e| e.name == name)
            .map(|e| (poly.vertices[e.a], poly.vertices[e.b]))
            .ok_or_else(|| format!("edge {name} does not exist on this part"))
    }
}

/// Centroid and outward normal of a planar face.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceFrame {
    pub centroid: Vec3,
    pub normal: Vec3,
    pub area: f64,
}

/// One polyhedron edge: vertex indices (in first-use direction) and its
/// construction name.
#[derive(Clone, Debug, PartialEq)]
pub struct PolyEdge {
    pub a: usize,
    pub b: usize,
    pub name: String,
}

/// Name of the edge between two faces: sorted, joined by `|`.
pub fn edge_name(a: &str, b: &str) -> String {
    if a == b {
        format!("{a}|seam")
    } else if a < b {
        format!("{a}|{b}")
    } else {
        format!("{b}|{a}")
    }
}

/// Appends `#2`, `#3`, ... to repeated names so every name is unique.
pub fn disambiguate(names: &mut [String]) {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for name in names.iter_mut() {
        let n = seen.entry(name.clone()).or_insert(0);
        *n += 1;
        if *n > 1 {
            *name = format!("{name}#{n}");
        }
    }
}

/// Polyhedra only; anything else reports that OpenCascade is required.
pub struct PolyhedralOracle;

impl ShapeOracle for PolyhedralOracle {
    fn integrals(&self, shape: &Shape) -> Result<MassIntegrals, String> {
        shape.volume_integrals().ok_or_else(|| {
            format!(
                "{} needs OpenCascade, which this build does not link",
                shape.needs_occt().unwrap_or("this shape")
            )
        })
    }

    fn native(&self) -> bool {
        false
    }
}

/// Closed polyhedral surface. Each face is a vertex loop, counter-clockwise
/// seen from outside, with a construction name (see module docs).
#[derive(Clone, Debug, PartialEq)]
pub struct Polyhedron {
    pub vertices: Vec<Vec3>,
    pub faces: Vec<PolyFace>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PolyFace {
    pub name: String,
    /// Outer boundary, counter-clockwise seen from outside.
    pub loop_: Vec<usize>,
    /// Inner boundaries, clockwise seen from outside.
    pub holes: Vec<Vec<usize>>,
}

impl PolyFace {
    /// Outer loop then every hole loop.
    pub fn loops(&self) -> impl Iterator<Item = &Vec<usize>> {
        std::iter::once(&self.loop_).chain(self.holes.iter())
    }
}

impl Polyhedron {
    pub fn cuboid(size: Vec3) -> Self {
        let [x, y, z] = size;
        let vertices = vec![
            [0.0, 0.0, 0.0],
            [x, 0.0, 0.0],
            [x, y, 0.0],
            [0.0, y, 0.0],
            [0.0, 0.0, z],
            [x, 0.0, z],
            [x, y, z],
            [0.0, y, z],
        ];
        let face = |name: &str, loop_: [usize; 4]| PolyFace {
            name: name.into(),
            loop_: loop_.to_vec(),
            holes: Vec::new(),
        };
        Self {
            vertices,
            faces: vec![
                face("z-", [0, 3, 2, 1]),
                face("z+", [4, 5, 6, 7]),
                face("y-", [0, 1, 5, 4]),
                face("y+", [2, 3, 7, 6]),
                face("x-", [0, 4, 7, 3]),
                face("x+", [1, 2, 6, 5]),
            ],
        }
    }

    /// Straight-sided prism of a normalized profile (outer counter-clockwise,
    /// holes clockwise). Holes become through-holes with their own walls.
    pub fn prism(profile: &Profile, depth: f64) -> Self {
        let mut vertices = Vec::new();
        // (bottom start index, count) per loop.
        let mut blocks = Vec::new();
        for path in std::iter::once(&profile.outer).chain(profile.holes.iter()) {
            let start = vertices.len();
            for v in path {
                vertices.push([v.x, v.y, 0.0]);
            }
            for v in path {
                vertices.push([v.x, v.y, depth]);
            }
            blocks.push((start, path.len()));
        }
        let bottom =
            |&(start, n): &(usize, usize)| -> Vec<usize> { (start..start + n).rev().collect() };
        let top =
            |&(start, n): &(usize, usize)| -> Vec<usize> { (start + n..start + 2 * n).collect() };
        let mut faces = vec![
            PolyFace {
                name: "bottom".into(),
                loop_: bottom(&blocks[0]),
                holes: blocks[1..].iter().map(bottom).collect(),
            },
            PolyFace {
                name: "top".into(),
                loop_: top(&blocks[0]),
                holes: blocks[1..].iter().map(top).collect(),
            },
        ];
        for (l, &(start, n)) in blocks.iter().enumerate() {
            for i in 0..n {
                let j = (i + 1) % n;
                faces.push(PolyFace {
                    name: if l == 0 {
                        format!("side.{i}")
                    } else {
                        format!("hole{l}.side.{i}")
                    },
                    loop_: vec![start + i, start + j, start + n + j, start + n + i],
                    holes: Vec::new(),
                });
            }
        }
        Self { vertices, faces }
    }

    pub fn transformed(&self, pose: &Pose) -> Self {
        Self {
            vertices: self.vertices.iter().map(|v| pose.apply(*v)).collect(),
            faces: self.faces.clone(),
        }
    }

    /// Edges in first-use order (faces in order, each loop in order), named
    /// after the two faces that share them. The kernel builds its B-rep
    /// edges in this same order, so names agree everywhere.
    pub fn edge_list(&self) -> Vec<PolyEdge> {
        let mut index: std::collections::HashMap<(usize, usize), usize> =
            std::collections::HashMap::new();
        let mut edges: Vec<(usize, usize, Vec<usize>)> = Vec::new();
        for (fi, face) in self.faces.iter().enumerate() {
            for l in face.loops() {
                for i in 0..l.len() {
                    let (a, b) = (l[i], l[(i + 1) % l.len()]);
                    let k = *index.entry((a.min(b), a.max(b))).or_insert_with(|| {
                        edges.push((a, b, Vec::new()));
                        edges.len() - 1
                    });
                    edges[k].2.push(fi);
                }
            }
        }
        let mut names: Vec<String> = edges
            .iter()
            .map(|(_, _, users)| match users.as_slice() {
                [f, g, ..] => edge_name(&self.faces[*f].name, &self.faces[*g].name),
                [f] => edge_name(&self.faces[*f].name, &self.faces[*f].name),
                [] => "edge".into(),
            })
            .collect();
        disambiguate(&mut names);
        edges
            .into_iter()
            .zip(names)
            .map(|((a, b, _), name)| PolyEdge { a, b, name })
            .collect()
    }

    /// Centroid, outward normal, and area of the named face.
    pub fn face_frame(&self, name: &str) -> Option<FaceFrame> {
        let face = self.faces.iter().find(|f| f.name == name)?;
        let pts: Vec<Vec3> = face.loop_.iter().map(|v| self.vertices[*v]).collect();
        let mut n = [0.0; 3];
        for i in 0..pts.len() {
            let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
            n[0] += (p[1] - q[1]) * (p[2] + q[2]);
            n[1] += (p[2] - q[2]) * (p[0] + q[0]);
            n[2] += (p[0] - q[0]) * (p[1] + q[1]);
        }
        let normal = normalize(n);
        let mut area = 0.0;
        let mut c = [0.0; 3];
        // Hole loops wind the other way, so their fans subtract.
        for l in face.loops() {
            let pts: Vec<Vec3> = l.iter().map(|v| self.vertices[*v]).collect();
            for w in pts[1..].windows(2) {
                let a = dot(cross(sub(w[0], pts[0]), sub(w[1], pts[0])), normal) * 0.5;
                area += a;
                for k in 0..3 {
                    c[k] += a * (pts[0][k] + w[0][k] + w[1][k]) / 3.0;
                }
            }
        }
        if area.abs() > 1e-15 {
            c = scale(c, 1.0 / area);
        }
        Some(FaceFrame {
            centroid: c,
            normal,
            area,
        })
    }

    pub fn scaled(&self, s: f64) -> Self {
        Self {
            vertices: self.vertices.iter().map(|v| scale(*v, s)).collect(),
            faces: self.faces.clone(),
        }
    }

    /// Unit-density mass integrals by the divergence theorem: the solid is
    /// the signed sum of tetrahedra from the origin to each fan triangle.
    /// Fan triangles of a non-convex planar face still integrate exactly
    /// because their signed coverage is 1 inside the face and 0 outside.
    pub fn integrals(&self) -> MassIntegrals {
        // Covariance of the canonical tetrahedron (0, e1, e2, e3) about the
        // origin, times 6 (det of the canonical frame is 1, volume 1/6).
        const C: Mat3 = [
            [1.0 / 60.0, 1.0 / 120.0, 1.0 / 120.0],
            [1.0 / 120.0, 1.0 / 60.0, 1.0 / 120.0],
            [1.0 / 120.0, 1.0 / 120.0, 1.0 / 60.0],
        ];
        let mut m = 0.0;
        let mut first = [0.0; 3];
        let mut second = [[0.0; 3]; 3];
        for l in self.faces.iter().flat_map(|f| f.loops()) {
            let p0 = self.vertices[l[0]];
            for w in l[1..].windows(2) {
                let (p1, p2) = (self.vertices[w[0]], self.vertices[w[1]]);
                let det = dot(p0, cross(p1, p2));
                m += det / 6.0;
                let s = add(add(p0, p1), p2);
                first = add(first, scale(s, det / 24.0));
                let a: Mat3 = transpose(&[p0, p1, p2]);
                let cov = mat_mul(&mat_mul(&a, &C), &transpose(&a));
                second = mat_add(&second, &cov, det);
            }
        }
        MassIntegrals {
            mass: m,
            first,
            second,
        }
    }
}

/// Linear mass integrals about a frame's origin: ∫ρ dV, ∫ρx dV, ∫ρ x xᵀ dV.
/// Linear, so composite parts are sums and differences of simple ones.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MassIntegrals {
    pub mass: f64,
    pub first: Vec3,
    pub second: Mat3,
}

impl MassIntegrals {
    pub fn scaled(&self, s: f64) -> Self {
        Self {
            mass: self.mass * s,
            first: scale(self.first, s),
            second: mat_add(&[[0.0; 3]; 3], &self.second, s),
        }
    }

    pub fn plus(&self, o: &Self) -> Self {
        Self {
            mass: self.mass + o.mass,
            first: add(self.first, o.first),
            second: mat_add(&self.second, &o.second, 1.0),
        }
    }

    pub fn minus(&self, o: &Self) -> Self {
        self.plus(&o.scaled(-1.0))
    }

    /// The same body expressed in the frame `pose` maps into.
    pub fn transformed(&self, pose: &Pose) -> Self {
        let r = pose.matrix();
        let t = pose.origin;
        let rf = mat_vec(&r, self.first);
        let rsr = mat_mul(&mat_mul(&r, &self.second), &transpose(&r));
        let mut second = rsr;
        second = mat_add(&second, &outer(rf, t), 1.0);
        second = mat_add(&second, &outer(t, rf), 1.0);
        second = mat_add(&second, &outer(t, t), self.mass);
        Self {
            mass: self.mass,
            first: add(rf, scale(t, self.mass)),
            second,
        }
    }

    pub fn props(&self) -> MassProps {
        MassProps::from_integrals(self)
    }
}

/// Mass properties in world axes. Inertia is about the centre of mass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MassProps {
    pub mass_lb: f64,
    pub volume_in3: f64,
    pub com: Vec3,
    /// lb·in², about `com`.
    pub inertia: Mat3,
    /// Principal moments, ascending, lb·in².
    pub principal: Vec3,
    /// Principal axes (unit vectors) matching `principal`.
    pub axes: [Vec3; 3],
    /// Linear integrals about the world origin, kept so assemblies combine
    /// exactly.
    pub integrals: MassIntegrals,
}

impl MassProps {
    pub fn from_integrals(i: &MassIntegrals) -> Self {
        let m = i.mass;
        let com = if m.abs() > 1e-12 {
            scale(i.first, 1.0 / m)
        } else {
            [0.0; 3]
        };
        // Second moment about the centre of mass, then I = tr(C)·E − C.
        let c = mat_add(&i.second, &outer(com, com), -m);
        let tr = c[0][0] + c[1][1] + c[2][2];
        let mut inertia = [[0.0; 3]; 3];
        for (a, row) in inertia.iter_mut().enumerate() {
            for (b, v) in row.iter_mut().enumerate() {
                *v = if a == b { tr } else { 0.0 } - c[a][b];
            }
        }
        let (principal, axes) = jacobi_eigen(inertia);
        Self {
            mass_lb: m,
            volume_in3: 0.0,
            com,
            inertia,
            principal,
            axes,
            integrals: *i,
        }
    }

    /// Mass properties of a uniform solid of `volume` integrals, density in
    /// lb/in³, placed by `pose`.
    pub fn uniform(volume: &MassIntegrals, density_lb_in3: f64, pose: &Pose) -> Self {
        let mut p = Self::from_integrals(&volume.scaled(density_lb_in3).transformed(pose));
        p.volume_in3 = volume.mass;
        p
    }

    pub fn combine<'a>(parts: impl IntoIterator<Item = &'a MassProps>) -> Self {
        let mut total = MassIntegrals::default();
        let mut volume = 0.0;
        for p in parts {
            total = total.plus(&p.integrals);
            volume += p.volume_in3;
        }
        let mut out = Self::from_integrals(&total);
        out.volume_in3 = volume;
        out
    }

    /// Moves the body rigidly by `pose`.
    pub fn transformed(&self, pose: &Pose) -> Self {
        let mut p = Self::from_integrals(&self.integrals.transformed(pose));
        p.volume_in3 = self.volume_in3;
        p
    }
}

/// Eigen-decomposition of a symmetric 3x3 matrix by cyclic Jacobi rotations.
/// Eigenvalues ascending, with matching unit eigenvectors.
pub fn jacobi_eigen(a: Mat3) -> (Vec3, [Vec3; 3]) {
    let mut a = a;
    let mut v: Mat3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..50 {
        let off = a[0][1].powi(2) + a[0][2].powi(2) + a[1][2].powi(2);
        let scale_ = a[0][0].abs() + a[1][1].abs() + a[2][2].abs();
        if off <= 1e-24 * scale_.max(1.0).powi(2) {
            break;
        }
        for (p, q) in [(0, 1), (0, 2), (1, 2)] {
            if a[p][q].abs() < 1e-300 {
                continue;
            }
            let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
            let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
            let t = if theta == 0.0 { 1.0 } else { t };
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            let mut j: Mat3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
            j[p][p] = c;
            j[q][q] = c;
            j[p][q] = s;
            j[q][p] = -s;
            a = mat_mul(&mat_mul(&transpose(&j), &a), &j);
            v = mat_mul(&v, &j);
        }
    }
    let mut pairs: Vec<(f64, Vec3)> = (0..3)
        .map(|k| (a[k][k], normalize([v[0][k], v[1][k], v[2][k]])))
        .collect();
    pairs.sort_by(|x, y| x.0.total_cmp(&y.0));
    (
        [pairs[0].0, pairs[1].0, pairs[2].0],
        [pairs[0].1, pairs[1].1, pairs[2].1],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * b.abs().max(1.0)
    }

    #[test]
    fn box_mass_properties_match_closed_form() {
        let (a, b, c) = (4.0, 6.0, 10.0);
        let i = Polyhedron::cuboid([a, b, c]).integrals();
        let p = MassProps::uniform(&i, 1.0, &Pose::IDENTITY);
        let m = a * b * c;
        assert!(close(p.mass_lb, m, 1e-12));
        assert!(close(p.com[0], 2.0, 1e-12) && close(p.com[2], 5.0, 1e-12));
        assert!(close(p.inertia[0][0], m * (b * b + c * c) / 12.0, 1e-12));
        assert!(close(p.inertia[1][1], m * (a * a + c * c) / 12.0, 1e-12));
        assert!(close(p.inertia[2][2], m * (a * a + b * b) / 12.0, 1e-12));
        assert!(p.inertia[0][1].abs() < 1e-9);
    }

    #[test]
    fn l_prism_matches_decomposition() {
        // L of a 6x2 bar and a 2x4 bar, 3 deep.
        let profile = Profile {
            outer: [[0., 0.], [6., 0.], [6., 2.], [2., 2.], [2., 6.], [0., 6.]]
                .iter()
                .map(|p| PathVertex::line(p[0], p[1]))
                .collect(),
            holes: vec![],
        };
        let shape = Shape::Prism {
            profile,
            depth: 3.0,
        };
        let l = shape.volume_integrals().unwrap();
        let bar = Polyhedron::cuboid([6.0, 2.0, 3.0]).integrals();
        let post = Polyhedron::cuboid([2.0, 4.0, 3.0])
            .integrals()
            .transformed(&Pose::translation([0.0, 2.0, 0.0]));
        let sum = bar.plus(&post);
        assert!(close(l.mass, 60.0, 1e-12));
        for k in 0..3 {
            assert!(close(l.first[k], sum.first[k], 1e-12));
            for j in 0..3 {
                assert!(close(l.second[k][j], sum.second[k][j], 1e-12));
            }
        }
    }

    #[test]
    fn rotation_keeps_principal_moments() {
        let i = Polyhedron::cuboid([2.0, 3.0, 7.0]).integrals();
        let a = MassProps::uniform(&i, 2.0, &Pose::IDENTITY);
        let pose = Pose::translation([5.0, -1.0, 2.0]).then(&Pose::rotation([1.0, 2.0, 0.5], 0.7));
        let b = MassProps::uniform(&i, 2.0, &pose);
        for k in 0..3 {
            assert!(close(a.principal[k], b.principal[k], 1e-9), "{a:?} {b:?}");
        }
        let expected = pose.apply(a.com);
        for (got, want) in b.com.iter().zip(expected) {
            assert!(close(*got, want, 1e-12));
        }
    }

    #[test]
    fn hollow_tube_is_outer_minus_inner() {
        let outer = Polyhedron::cuboid([2.0, 4.0, 100.0]).integrals();
        let inner = Polyhedron::cuboid([1.75, 3.75, 100.0])
            .integrals()
            .transformed(&Pose::translation([0.125, 0.125, 0.0]));
        let tube = outer.minus(&inner).props();
        assert!(close(tube.mass_lb, (8.0 - 1.75 * 3.75) * 100.0, 1e-12));
        assert!(close(tube.com[0], 1.0, 1e-12) && close(tube.com[1], 2.0, 1e-12));
    }

    #[test]
    fn pose_inverse_and_composition() {
        let a = Pose::translation([1.0, 2.0, 3.0]).then(&Pose::rotation([0.0, 0.0, 1.0], 0.3));
        let b = Pose::rotation([1.0, 0.0, 0.0], -1.1);
        let p = [0.4, -2.0, 5.0];
        let ab = a.then(&b);
        let direct = a.apply(b.apply(p));
        let composed = ab.apply(p);
        for k in 0..3 {
            assert!(close(direct[k], composed[k], 1e-12));
        }
        let back = a.inverse().apply(a.apply(p));
        for k in 0..3 {
            assert!(close(back[k], p[k], 1e-12));
        }
        let hinge = Pose::rotation_about(
            [0.0, 10.0, 0.0],
            [1.0, 0.0, 0.0],
            std::f64::consts::FRAC_PI_2,
        );
        let q = hinge.apply([0.0, 20.0, 0.0]);
        assert!(close(q[1], 10.0, 1e-12) && close(q[2], 10.0, 1e-12));
    }

    #[test]
    fn frames_from_axes_round_trip() {
        let p = Pose::from_axes(
            [1.0, 2.0, 3.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
        );
        let q = p.apply([2.0, 3.0, 4.0]);
        assert!(close(q[0], 5.0, 1e-12) && close(q[1], 4.0, 1e-12) && close(q[2], 6.0, 1e-12));
        let xz = Pose::from_axes([0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]);
        let n = xz.rotate([0.0, 0.0, 1.0]);
        assert!(close(n[1], -1.0, 1e-12));
    }

    #[test]
    fn named_edges_and_faces() {
        let poly = Polyhedron::cuboid([2.0, 3.0, 4.0]);
        let edges = poly.edge_list();
        assert_eq!(edges.len(), 12);
        let e = edges.iter().find(|e| e.name == "y-|z+").unwrap();
        let (a, b) = (poly.vertices[e.a], poly.vertices[e.b]);
        assert!(close(norm(sub(b, a)), 2.0, 1e-12));
        assert!(close(a[1], 0.0, 1e-12) && close(a[2], 4.0, 1e-12));
        let f = poly.face_frame("x+").unwrap();
        assert!(close(f.normal[0], 1.0, 1e-12) && close(f.area, 12.0, 1e-12));
        assert!(close(f.centroid[1], 1.5, 1e-12));
    }

    #[test]
    fn profile_validation_explains_failures() {
        let bow = Profile {
            outer: [[0., 0.], [4., 4.], [4., 0.], [0., 4.]]
                .iter()
                .map(|p| PathVertex::line(p[0], p[1]))
                .collect(),
            holes: vec![],
        };
        let err = bow.normalized().unwrap_err();
        assert!(err.contains("crosses itself at (2, 2)"), "{err}");

        let mut cw = Profile::rectangle(3.0, 2.0);
        cw.outer.reverse();
        let n = cw.normalized().unwrap();
        assert!(signed_area(&n.outer.iter().map(|v| v.p()).collect::<Vec<_>>()) > 0.0);

        let mut tiny = Profile::rectangle(3.0, 2.0);
        tiny.outer.push(PathVertex::line(0.0, 2.0005));
        assert!(tiny.normalized().unwrap_err().contains("zero-length"));
    }

    #[test]
    fn arcs_have_exact_area_and_need_occt() {
        // A 2 in diameter circle as two half-circle bulges.
        let circle = Profile {
            outer: vec![
                PathVertex {
                    x: -1.0,
                    y: 0.0,
                    bulge: 1.0,
                },
                PathVertex {
                    x: 1.0,
                    y: 0.0,
                    bulge: 1.0,
                },
            ],
            holes: vec![],
        };
        assert!(close(circle.area(), std::f64::consts::PI, 1e-12));
        let shape = Shape::Prism {
            profile: circle,
            depth: 1.0,
        };
        shape.validate().unwrap();
        assert_eq!(shape.needs_occt(), Some("arcs in a profile"));
        assert!(shape.polyhedron().is_none());
    }

    #[test]
    fn tube_prism_has_through_hole_walls() {
        let profile = Profile {
            outer: Profile::rectangle(2.0, 4.0).outer,
            holes: vec![[
                [0.125, 0.125],
                [0.125, 3.875],
                [1.875, 3.875],
                [1.875, 0.125],
            ]
            .iter()
            .map(|p| PathVertex::line(p[0], p[1]))
            .collect()],
        }
        .normalized()
        .unwrap();
        let shape = Shape::Prism {
            profile,
            depth: 100.0,
        };
        assert_eq!(shape.needs_occt(), None);
        let poly = shape.polyhedron().unwrap();
        assert_eq!(poly.faces.len(), 10);
        let i = poly.integrals();
        assert!(close(i.mass, (8.0 - 1.75 * 3.75) * 100.0, 1e-12));
        let top = poly.face_frame("top").unwrap();
        assert!(close(top.area, 8.0 - 1.75 * 3.75, 1e-12));
        assert!(close(top.centroid[0], 1.0, 1e-12) && close(top.centroid[1], 2.0, 1e-12));
        assert!(poly
            .edge_list()
            .iter()
            .any(|e| e.name == "hole1.side.0|top"));
    }

    #[test]
    fn holes_triangulate_to_the_net_area() {
        let outer = [[0., 0.], [4., 0.], [4., 4.], [0., 4.]];
        let holes = vec![vec![[1., 1.], [1., 3.], [3., 3.], [3., 1.]]];
        let tris = triangulate_with_holes(&outer, &holes);
        let mut all: Vec<[f64; 2]> = outer.to_vec();
        all.extend_from_slice(&holes[0]);
        let area: f64 = tris
            .iter()
            .map(|t| signed_area(&[all[t[0]], all[t[1]], all[t[2]]]))
            .sum();
        assert!(close(area, 12.0, 1e-12), "{area}");
        assert!(tris
            .iter()
            .all(|t| signed_area(&[all[t[0]], all[t[1]], all[t[2]]]) > 0.0));
    }

    #[test]
    fn ear_clipping_handles_concave_polygons() {
        let p = [[0., 0.], [6., 0.], [6., 2.], [2., 2.], [2., 6.], [0., 6.]];
        let tris = triangulate(&p);
        assert_eq!(tris.len(), 4);
        let area: f64 = tris
            .iter()
            .map(|t| signed_area(&[p[t[0]], p[t[1]], p[t[2]]]))
            .sum();
        assert!(close(area, 20.0, 1e-12));
        assert!(tris
            .iter()
            .all(|t| signed_area(&[p[t[0]], p[t[1]], p[t[2]]]) > 0.0));
    }
}
