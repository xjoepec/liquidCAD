//! Boundary representation for planar polyhedral solids.
//!
//! This is the exact geometry the fallback kernel exports to STEP. Display
//! meshes are derived from it by [`BrepSolid::tessellate`] and never feed back
//! into it.

use liquidcad_core::document::{CurveKind, EdgeInfo, FaceInfo, SolidBox, SurfaceKind};
use liquidcad_core::geometry::{triangulate_with_holes, Polyhedron};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct BrepSolid {
    pub vertices: Vec<[f64; 3]>,
    /// Straight edges, start vertex to end vertex.
    pub edges: Vec<(usize, usize)>,
    /// Construction name of each edge (see `liquidcad_core::geometry`).
    pub edge_names: Vec<String>,
    pub faces: Vec<BrepFace>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BrepFace {
    /// Construction name, e.g. `z+` or `side.3`.
    pub name: String,
    /// Outward unit normal of the supporting plane.
    pub normal: [f64; 3],
    /// In-plane reference direction for the plane placement.
    pub ref_dir: [f64; 3],
    /// Outer loop, counter-clockwise seen from outside: (edge, forward).
    pub outer: Vec<(usize, bool)>,
    /// Inner loops (holes), clockwise seen from outside.
    pub inner: Vec<Vec<(usize, bool)>>,
}

/// Display data derived from a B-rep: triangles grouped by face, the edge
/// wire, and per-face / per-edge metadata. Units are the caller's (inches).
pub struct Display {
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub indices: Vec<u32>,
    pub wire: Vec<f32>,
    pub faces: Vec<FaceInfo>,
    pub edges: Vec<EdgeInfo>,
}

pub use liquidcad_core::geometry::{disambiguate, edge_name};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum BrepError {
    #[error("Euler-Poincare V-E+F-R = {0} is not 2 - 2g for a whole genus g")]
    Euler(i64),
    #[error("edge {0} is used {1} times; a closed manifold shell uses each edge twice")]
    EdgeUse(usize, usize),
    #[error("edge {0} is traversed in the same direction by both faces")]
    EdgeOrientation(usize),
    #[error("face {0} loop is not closed")]
    OpenLoop(usize),
    #[error("face {0} loop winds against its normal")]
    Winding(usize),
    #[error("solid has non-positive volume {0}")]
    Volume(f64),
}

impl BrepSolid {
    /// Axis-aligned box, scaled by `scale` (25.4 turns inches into mm).
    pub fn from_box(b: SolidBox, scale: f64) -> Self {
        let poly = Polyhedron::cuboid(b.size)
            .transformed(&liquidcad_core::geometry::Pose::translation(b.origin));
        Self::from_polyhedron(&poly.scaled(scale))
    }

    /// B-rep of a closed polyhedron whose faces are counter-clockwise from
    /// outside. Edges are shared between the two faces that use them.
    pub fn from_polyhedron(poly: &Polyhedron) -> Self {
        let list = poly.edge_list();
        let edges: Vec<(usize, usize)> = list.iter().map(|e| (e.a, e.b)).collect();
        let edge_names: Vec<String> = list.into_iter().map(|e| e.name).collect();
        let index: HashMap<(usize, usize), usize> = edges
            .iter()
            .enumerate()
            .map(|(i, (a, b))| ((*a.min(b), *a.max(b)), i))
            .collect();
        let mut faces = Vec::with_capacity(poly.faces.len());
        let oriented = |l: &Vec<usize>| -> Vec<(usize, bool)> {
            (0..l.len())
                .map(|i| {
                    let (a, b) = (l[i], l[(i + 1) % l.len()]);
                    let e = index[&(a.min(b), a.max(b))];
                    (e, edges[e] == (a, b))
                })
                .collect()
        };
        for face in &poly.faces {
            let l = &face.loop_;
            let outer = oriented(l);
            let inner: Vec<Vec<(usize, bool)>> = face.holes.iter().map(oriented).collect();
            let pts: Vec<[f64; 3]> = l.iter().map(|v| poly.vertices[*v]).collect();
            let normal = unit(newell(&pts));
            let ref_dir = unit(sub(pts[1], pts[0]));
            faces.push(BrepFace {
                name: face.name.clone(),
                normal,
                ref_dir,
                outer,
                inner,
            });
        }
        Self {
            vertices: poly.vertices.clone(),
            edges,
            edge_names,
            faces,
        }
    }

    fn oriented(&self, (edge, forward): (usize, bool)) -> (usize, usize) {
        let (a, b) = self.edges[edge];
        if forward {
            (a, b)
        } else {
            (b, a)
        }
    }

    /// Vertex indices around a face's outer loop.
    pub fn loop_vertices(&self, face: &BrepFace) -> Vec<usize> {
        self.ring(&face.outer)
    }

    /// Vertex indices around any loop.
    pub fn ring(&self, edges: &[(usize, bool)]) -> Vec<usize> {
        edges.iter().map(|oe| self.oriented(*oe).0).collect()
    }

    fn all_loops(face: &BrepFace) -> impl Iterator<Item = &Vec<(usize, bool)>> {
        std::iter::once(&face.outer).chain(face.inner.iter())
    }

    /// Point on the face plane (first loop vertex).
    pub fn face_origin(&self, face: &BrepFace) -> [f64; 3] {
        self.vertices[self.oriented(face.outer[0]).0]
    }

    /// Topological and geometric validity of a closed, oriented shell:
    /// Euler-Poincare (V - E + F - R = 2 - 2g, R inner loops, g through
    /// holes), every edge used once each way, closed loops, outer loops
    /// counter-clockwise and inner loops clockwise about the face normal,
    /// and positive volume.
    pub fn validate(&self) -> Result<(), BrepError> {
        let rings: usize = self.faces.iter().map(|f| f.inner.len()).sum();
        let euler = self.vertices.len() as i64 - self.edges.len() as i64 + self.faces.len() as i64
            - rings as i64;
        if euler > 2 || (2 - euler) % 2 != 0 {
            return Err(BrepError::Euler(euler));
        }
        let mut uses = vec![(0usize, 0usize); self.edges.len()];
        for (fi, face) in self.faces.iter().enumerate() {
            for (k, ring) in Self::all_loops(face).enumerate() {
                for (i, oe) in ring.iter().enumerate() {
                    let (_, end) = self.oriented(*oe);
                    let (next_start, _) = self.oriented(ring[(i + 1) % ring.len()]);
                    if end != next_start {
                        return Err(BrepError::OpenLoop(fi));
                    }
                    if oe.1 {
                        uses[oe.0].0 += 1;
                    } else {
                        uses[oe.0].1 += 1;
                    }
                }
                let n = newell(
                    &self
                        .ring(ring)
                        .iter()
                        .map(|v| self.vertices[*v])
                        .collect::<Vec<_>>(),
                );
                let along = dot(n, face.normal);
                if (k == 0 && along <= 0.0) || (k > 0 && along >= 0.0) {
                    return Err(BrepError::Winding(fi));
                }
            }
        }
        for (edge, (fwd, rev)) in uses.iter().enumerate() {
            if fwd + rev != 2 {
                return Err(BrepError::EdgeUse(edge, fwd + rev));
            }
            if *fwd != 1 {
                return Err(BrepError::EdgeOrientation(edge));
            }
        }
        let volume = self.volume();
        if volume <= 0.0 {
            return Err(BrepError::Volume(volume));
        }
        Ok(())
    }

    /// Signed volume from the divergence theorem over fan triangles of
    /// every loop (holes wind the other way and subtract).
    pub fn volume(&self) -> f64 {
        let mut six_v = 0.0;
        for face in &self.faces {
            for ring in Self::all_loops(face) {
                let loop_v = self.ring(ring);
                let p0 = self.vertices[loop_v[0]];
                for w in loop_v[1..].windows(2) {
                    let (p1, p2) = (self.vertices[w[0]], self.vertices[w[1]]);
                    six_v += dot(p0, cross(p1, p2));
                }
            }
        }
        six_v / 6.0
    }

    /// Flat-shaded triangles (ear-clipped per planar face) and the edge
    /// wire. Coordinates are divided by `scale` to return to display units.
    pub fn tessellate(&self, scale: f64) -> (Vec<f32>, Vec<f32>, Vec<u32>, Vec<f32>) {
        let d = self.display(scale);
        (d.positions, d.normals, d.indices, d.wire)
    }

    /// Triangles grouped by face plus face and edge metadata, in display
    /// units (`scale` divides the B-rep coordinates).
    pub fn display(&self, scale: f64) -> Display {
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut faces = Vec::with_capacity(self.faces.len());
        for face in &self.faces {
            let mut loop_v = self.loop_vertices(face);
            let outer_len = loop_v.len();
            let hole_rings: Vec<Vec<usize>> = face.inner.iter().map(|r| self.ring(r)).collect();
            for r in &hole_rings {
                loop_v.extend_from_slice(r);
            }
            let pts: Vec<[f64; 3]> = loop_v.iter().map(|v| self.vertices[*v]).collect();
            let base = (positions.len() / 3) as u32;
            for p in &pts {
                for c in p {
                    positions.push((c / scale) as f32);
                }
                for c in face.normal {
                    normals.push(c as f32);
                }
            }
            // Project onto the face plane and ear-clip so concave faces
            // (an L-shaped bracket's top) triangulate correctly.
            let u = face.ref_dir;
            let v = cross(face.normal, u);
            let flat: Vec<[f64; 2]> = pts.iter().map(|p| [dot(*p, u), dot(*p, v)]).collect();
            let mut holes2d = Vec::with_capacity(hole_rings.len());
            let mut at = outer_len;
            let mut hole_pts = Vec::new();
            for r in &hole_rings {
                holes2d.push(flat[at..at + r.len()].to_vec());
                hole_pts.push(pts[at..at + r.len()].to_vec());
                at += r.len();
            }
            let tri_start = (indices.len() / 3) as u32;
            for t in triangulate_with_holes(&flat[..outer_len], &holes2d) {
                indices.extend(t.iter().map(|k| base + *k as u32));
            }
            let (mut area, outer_c) = polygon_area_centroid(&pts[..outer_len], face.normal);
            let mut moment = outer_c.map(|c| c * area);
            for hp in &hole_pts {
                // Clockwise about the normal: negative area.
                let (a, c) = polygon_area_centroid(hp, face.normal);
                area += a;
                for k in 0..3 {
                    moment[k] += c[k] * a;
                }
            }
            let centroid = if area.abs() > 1e-15 {
                moment.map(|m| m / area)
            } else {
                outer_c
            };
            faces.push(FaceInfo {
                id: face.name.clone(),
                kind: SurfaceKind::Plane,
                area_in2: area / (scale * scale),
                centroid: centroid.map(|c| c / scale),
                normal: face.normal,
                tri_start,
                tri_count: (indices.len() / 3) as u32 - tri_start,
            });
        }
        let mut wire = Vec::with_capacity(self.edges.len() * 6);
        let mut edges = Vec::with_capacity(self.edges.len());
        for ((a, b), id) in self.edges.iter().zip(self.edge_names.iter().cloned()) {
            let (pa, pb) = (self.vertices[*a], self.vertices[*b]);
            let polyline: Vec<f32> = pa
                .iter()
                .chain(pb.iter())
                .map(|c| (c / scale) as f32)
                .collect();
            wire.extend_from_slice(&polyline);
            edges.push(EdgeInfo {
                id,
                kind: CurveKind::Line,
                length_in: norm(sub(pb, pa)) / scale,
                polyline,
            });
        }
        Display {
            positions,
            normals,
            indices,
            wire,
            faces,
            edges,
        }
    }
}

/// Area and centroid of a planar polygon with the given unit normal.
fn polygon_area_centroid(pts: &[[f64; 3]], normal: [f64; 3]) -> (f64, [f64; 3]) {
    let mut area = 0.0;
    let mut c = [0.0; 3];
    let p0 = pts[0];
    for w in pts[1..].windows(2) {
        let a = dot(cross(sub(w[0], p0), sub(w[1], p0)), normal) * 0.5;
        area += a;
        for k in 0..3 {
            c[k] += a * (p0[k] + w[0][k] + w[1][k]) / 3.0;
        }
    }
    if area.abs() > 1e-15 {
        c = c.map(|v| v / area);
    }
    (area, c)
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

fn unit(a: [f64; 3]) -> [f64; 3] {
    let n = norm(a);
    if n > 0.0 {
        [a[0] / n, a[1] / n, a[2] / n]
    } else {
        a
    }
}

pub(crate) fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn newell(points: &[[f64; 3]]) -> [f64; 3] {
    let mut n = [0.0; 3];
    for i in 0..points.len() {
        let (p, q) = (points[i], points[(i + 1) % points.len()]);
        n[0] += (p[1] - q[1]) * (p[2] + q[2]);
        n[1] += (p[2] - q[2]) * (p[0] + q[0]);
        n[2] += (p[0] - q[0]) * (p[1] + q[1]);
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> BrepSolid {
        BrepSolid::from_box(
            SolidBox {
                origin: [1.0, 2.0, 3.0],
                size: [4.0, 5.0, 6.0],
            },
            1.0,
        )
    }

    #[test]
    fn box_is_a_valid_closed_oriented_shell() {
        let solid = sample();
        solid.validate().unwrap();
        assert!((solid.volume() - 120.0).abs() < 1e-9);
    }

    #[test]
    fn scaling_to_mm_scales_volume_by_the_cube() {
        let solid = BrepSolid::from_box(
            SolidBox {
                origin: [0.0; 3],
                size: [1.0, 1.0, 1.0],
            },
            25.4,
        );
        assert!((solid.volume() - 25.4f64.powi(3)).abs() < 1e-6);
    }

    #[test]
    fn validation_catches_flipped_faces() {
        let mut solid = sample();
        solid.faces[0].outer.reverse();
        for oe in &mut solid.faces[0].outer {
            oe.1 = !oe.1;
        }
        assert!(solid.validate().is_err());
    }

    #[test]
    fn concave_prism_is_valid_and_named() {
        use liquidcad_core::geometry::{PathVertex, Profile};
        let profile = Profile {
            outer: [[0., 0.], [6., 0.], [6., 2.], [2., 2.], [2., 6.], [0., 6.]]
                .iter()
                .map(|p| PathVertex::line(p[0], p[1]))
                .collect(),
            holes: vec![],
        };
        let solid = BrepSolid::from_polyhedron(&Polyhedron::prism(&profile, 3.0));
        solid.validate().unwrap();
        assert!((solid.volume() - 60.0).abs() < 1e-9);
        let d = solid.display(1.0);
        let top = d.faces.iter().find(|f| f.id == "top").unwrap();
        assert!((top.area_in2 - 20.0).abs() < 1e-9);
        assert_eq!(top.tri_count, 4);
        assert_eq!(d.faces.len(), 8);
        assert_eq!(d.edges.len(), 18);
        assert!(d.edges.iter().any(|e| e.id == "side.0|top"));
        assert!(d.edges.iter().any(|e| e.id == "side.0|side.1"));
        let total: f64 = d.faces.iter().map(|f| f.area_in2).sum();
        assert!((total - (2.0 * 20.0 + 24.0 * 3.0)).abs() < 1e-9);
    }

    #[test]
    fn tessellation_normals_match_triangle_winding() {
        let (pos, nrm, idx, edges) = sample().tessellate(1.0);
        assert_eq!(idx.len(), 36);
        assert_eq!(edges.len(), 12 * 6);
        for tri in idx.chunks(3) {
            let p = |i: u32| {
                let i = i as usize * 3;
                [pos[i] as f64, pos[i + 1] as f64, pos[i + 2] as f64]
            };
            let (a, b, c) = (p(tri[0]), p(tri[1]), p(tri[2]));
            let n = cross(
                [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
                [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
            );
            let i = tri[0] as usize * 3;
            let vn = [nrm[i] as f64, nrm[i + 1] as f64, nrm[i + 2] as f64];
            assert!(dot(n, vn) > 0.0, "triangle winds against its normal");
        }
    }
}
