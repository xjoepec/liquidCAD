//! Safe wrapper over `cpp/occt_adapter.cpp`. Only compiled with `cfg(occt)`.
//!
//! Shapes are built in the component's local frame, in inches. Every built
//! shape carries face and edge names derived from the adapter's provenance
//! tags, using the same scheme as the Rust B-rep (`x+`, `top`, `side.3`,
//! `fillet(side.0|top)`, ...), so references survive modifier operations.

use crate::brep::{disambiguate, edge_name};
use crate::KernelError;
use liquidcad_core::geometry::{sub, MassIntegrals, Pose, Profile, Shape, ShapeOp, Vec3};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::path::Path;
use std::ptr;

#[repr(C)]
struct LcSession {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct LcFace {
    pub surface: c_int,
    pub tag: c_int,
    pub index: c_int,
    pub area: f64,
    pub centroid: [f64; 3],
    pub normal: [f64; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct LcEdge {
    pub curve: c_int,
    pub face_a: c_int,
    pub face_b: c_int,
    pub length: f64,
}

#[repr(C)]
struct LcMesh {
    positions: *mut f32,
    normals: *mut f32,
    indices: *mut u32,
    face_tri_start: *mut u32,
    vertex_count: usize,
    index_count: usize,
    face_count: usize,
}

const TAG_BOTTOM: c_int = 1;
const TAG_TOP: c_int = 2;
const TAG_SIDE: c_int = 3;
const TAG_BOX: c_int = 4;
const TAG_KEPT: c_int = 5;
const TAG_FROM_EDGE: c_int = 6;
const TAG_HOLE: c_int = 7;
const TAG_TOOL: c_int = 8;
const TAG_REV_SIDE: c_int = 9;
const TAG_REV_START: c_int = 10;
const TAG_REV_END: c_int = 11;

extern "C" {
    fn lc_kernel_version(buffer: *mut c_char, cap: usize) -> c_int;
    fn lc_session_new() -> *mut LcSession;
    fn lc_session_free(session: *mut LcSession);
    fn lc_last_error(session: *const LcSession) -> *const c_char;
    fn lc_profile_begin(session: *mut LcSession, plane: c_int);
    fn lc_profile_loop(session: *mut LcSession, xyb: *const f64, n: usize) -> c_int;
    fn lc_make_box(session: *mut LcSession, size: *const f64) -> c_int;
    fn lc_make_prism(session: *mut LcSession, depth: f64) -> c_int;
    fn lc_make_revol(session: *mut LcSession, angle_deg: f64) -> c_int;
    fn lc_fillet(
        session: *mut LcSession,
        shape: c_int,
        edges: *const c_int,
        n: usize,
        radius: f64,
    ) -> c_int;
    fn lc_chamfer(
        session: *mut LcSession,
        shape: c_int,
        edges: *const c_int,
        n: usize,
        distance: f64,
    ) -> c_int;
    fn lc_hole(
        session: *mut LcSession,
        shape: c_int,
        origin: *const f64,
        dir: *const f64,
        diameter: f64,
        depth: f64,
    ) -> c_int;
    fn lc_cut(session: *mut LcSession, shape: c_int, tool: c_int) -> c_int;
    fn lc_transform(session: *mut LcSession, shape: c_int, m: *const f64, scale: f64) -> c_int;
    fn lc_release(session: *mut LcSession, shape: c_int);
    fn lc_check(session: *mut LcSession, shape: c_int) -> c_int;
    fn lc_face_count(session: *mut LcSession, shape: c_int) -> c_int;
    fn lc_edge_count(session: *mut LcSession, shape: c_int) -> c_int;
    fn lc_topology(
        session: *mut LcSession,
        shape: c_int,
        faces: *mut LcFace,
        edges: *mut LcEdge,
    ) -> c_int;
    fn lc_edge_polyline(
        session: *mut LcSession,
        shape: c_int,
        edge: c_int,
        out: *mut f64,
        cap: usize,
    ) -> c_int;
    fn lc_mass(session: *mut LcSession, shape: c_int, out: *mut f64) -> c_int;
    fn lc_tessellate(
        session: *mut LcSession,
        shape: c_int,
        deflection: f64,
        out: *mut LcMesh,
    ) -> c_int;
    fn lc_mesh_free(mesh: *mut LcMesh);
    fn lc_write_step(
        session: *mut LcSession,
        shapes: *const c_int,
        names: *const *const c_char,
        rgb: *const f64,
        n: usize,
        path: *const c_char,
        product: *const c_char,
    ) -> c_int;
}

pub fn version() -> String {
    let mut buffer = [0 as c_char; 96];
    // SAFETY: buffer is writable for its full length; the adapter NUL-terminates.
    unsafe {
        lc_kernel_version(buffer.as_mut_ptr(), buffer.len());
        CStr::from_ptr(buffer.as_ptr())
            .to_string_lossy()
            .into_owned()
    }
}

/// A built shape: session id, topology, and construction names.
pub struct Built {
    pub id: c_int,
    pub faces: Vec<LcFace>,
    pub edges: Vec<LcEdge>,
    pub face_names: Vec<String>,
    pub edge_names: Vec<String>,
}

impl Built {
    fn face(&self, name: &str) -> Option<&LcFace> {
        self.face_names
            .iter()
            .position(|n| n == name)
            .map(|i| &self.faces[i])
    }
}

pub struct MeshData {
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub indices: Vec<u32>,
    /// First triangle of each face, plus a final end marker.
    pub face_tri_start: Vec<u32>,
}

pub struct Session(*mut LcSession);

// SAFETY: a session is only ever used from one thread at a time (it lives
// inside the engine, which owns it); OCCT objects are not shared across
// sessions.
unsafe impl Send for Session {}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: pointer came from lc_session_new and is freed once.
        unsafe { lc_session_free(self.0) }
    }
}

impl Session {
    pub fn new() -> Result<Self, KernelError> {
        // SAFETY: plain constructor; null is checked.
        let ptr = unsafe { lc_session_new() };
        if ptr.is_null() {
            return Err(KernelError::Native("could not allocate session".into()));
        }
        Ok(Self(ptr))
    }

    fn error(&self) -> KernelError {
        // SAFETY: session is live; the adapter returns a NUL-terminated string it owns.
        let msg = unsafe { CStr::from_ptr(lc_last_error(self.0)) };
        KernelError::Native(msg.to_string_lossy().into_owned())
    }

    fn id(&self, id: c_int) -> Result<c_int, KernelError> {
        if id < 0 {
            Err(self.error())
        } else {
            Ok(id)
        }
    }

    pub fn release(&mut self, id: c_int) {
        // SAFETY: release tolerates any id.
        unsafe { lc_release(self.0, id) }
    }

    fn profile(&mut self, profile: &Profile, plane: c_int) -> Result<(), KernelError> {
        let p = profile.normalized().map_err(KernelError::Native)?;
        // SAFETY: plain call on a live session.
        unsafe { lc_profile_begin(self.0, plane) };
        for path in std::iter::once(&p.outer).chain(p.holes.iter()) {
            let flat: Vec<f64> = path.iter().flat_map(|v| [v.x, v.y, v.bulge]).collect();
            // SAFETY: flat holds 3 * path.len() doubles and outlives the call.
            if unsafe { lc_profile_loop(self.0, flat.as_ptr(), path.len()) } != 0 {
                return Err(self.error());
            }
        }
        Ok(())
    }

    fn topology(&mut self, id: c_int) -> Result<(Vec<LcFace>, Vec<LcEdge>), KernelError> {
        // SAFETY: counts are queried on a live id.
        let (nf, ne) = unsafe { (lc_face_count(self.0, id), lc_edge_count(self.0, id)) };
        if nf < 0 || ne < 0 {
            return Err(self.error());
        }
        let mut faces = vec![LcFace::default(); nf as usize];
        let mut edges = vec![LcEdge::default(); ne as usize];
        // SAFETY: buffers are sized to the counts just returned.
        if unsafe { lc_topology(self.0, id, faces.as_mut_ptr(), edges.as_mut_ptr()) } != 0 {
            return Err(self.error());
        }
        Ok((faces, edges))
    }

    fn finish(
        &mut self,
        id: c_int,
        name_face: impl Fn(usize, &LcFace) -> String,
    ) -> Result<Built, KernelError> {
        // SAFETY: id is live.
        if unsafe { lc_check(self.0, id) } != 1 {
            return Err(KernelError::Native(
                "BRepCheck_Analyzer rejected the solid".into(),
            ));
        }
        let (faces, edges) = self.topology(id)?;
        let mut face_names: Vec<String> = faces
            .iter()
            .enumerate()
            .map(|(i, f)| name_face(i, f))
            .collect();
        disambiguate(&mut face_names);
        let mut edge_names: Vec<String> = edges
            .iter()
            .map(|e| {
                let name = |k: c_int| {
                    usize::try_from(k)
                        .ok()
                        .and_then(|k| face_names.get(k))
                        .map(String::as_str)
                        .unwrap_or("?")
                };
                edge_name(name(e.face_a), name(e.face_b))
            })
            .collect();
        disambiguate(&mut edge_names);
        Ok(Built {
            id,
            faces,
            edges,
            face_names,
            edge_names,
        })
    }

    /// Builds `shape` in its local frame.
    pub fn build(&mut self, shape: &Shape) -> Result<Built, KernelError> {
        match shape {
            Shape::Box { size } => {
                // SAFETY: size holds three doubles.
                let id = self.id(unsafe { lc_make_box(self.0, size.as_ptr()) })?;
                const NAMES: [&str; 6] = ["x-", "x+", "y-", "y+", "z-", "z+"];
                self.finish(id, |i, f| match f.tag {
                    TAG_BOX => NAMES
                        .get(f.index as usize)
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("face.{i}")),
                    _ => format!("face.{i}"),
                })
            }
            Shape::Prism { profile, depth } => {
                self.profile(profile, 0)?;
                // SAFETY: a profile was just set.
                let id = self.id(unsafe { lc_make_prism(self.0, *depth) })?;
                self.finish(id, |i, f| sweep_name(i, f, "side"))
            }
            Shape::Revolve { profile, angle_deg } => {
                self.profile(profile, 1)?;
                // SAFETY: a profile was just set.
                let id = self.id(unsafe { lc_make_revol(self.0, *angle_deg) })?;
                self.finish(id, |i, f| sweep_name(i, f, "rev"))
            }
            Shape::Modified { base, ops } => {
                let mut current = self.build(base)?;
                for (k, op) in ops.iter().enumerate() {
                    let next = self.apply(&current, k + 1, op);
                    self.release(current.id);
                    current = next?;
                }
                Ok(current)
            }
        }
    }

    fn edge_indices(&self, input: &Built, names: &[String]) -> Result<Vec<c_int>, KernelError> {
        names
            .iter()
            .map(|n| {
                input
                    .edge_names
                    .iter()
                    .position(|e| e == n)
                    .map(|i| i as c_int)
                    .ok_or_else(|| {
                        KernelError::Native(format!("edge {n} no longer exists on this part"))
                    })
            })
            .collect()
    }

    fn apply(&mut self, input: &Built, k: usize, op: &ShapeOp) -> Result<Built, KernelError> {
        let kept = |f: &LcFace| input.face_names.get(f.index as usize).cloned();
        match op {
            ShapeOp::Fillet { edges, radius }
            | ShapeOp::Chamfer {
                edges,
                distance: radius,
            } => {
                let fillet = matches!(op, ShapeOp::Fillet { .. });
                let idx = self.edge_indices(input, edges)?;
                // SAFETY: idx holds idx.len() ints and outlives the call.
                let id = self.id(unsafe {
                    if fillet {
                        lc_fillet(self.0, input.id, idx.as_ptr(), idx.len(), *radius)
                    } else {
                        lc_chamfer(self.0, input.id, idx.as_ptr(), idx.len(), *radius)
                    }
                })?;
                let label = if fillet { "fillet" } else { "chamfer" };
                self.finish(id, |i, f| match f.tag {
                    TAG_KEPT => kept(f).unwrap_or_else(|| format!("face.{i}")),
                    TAG_FROM_EDGE => format!(
                        "{label}({})",
                        input
                            .edge_names
                            .get(f.index as usize)
                            .map(String::as_str)
                            .unwrap_or("?")
                    ),
                    _ => format!("{label}{k}.face.{i}"),
                })
            }
            ShapeOp::Hole {
                face,
                at,
                diameter,
                depth,
            } => {
                let target = input.face(face).ok_or_else(|| {
                    KernelError::Native(format!("face {face} no longer exists on this part"))
                })?;
                if target.surface != 0 {
                    return Err(KernelError::Native(format!(
                        "holes go into flat faces; {face} is curved"
                    )));
                }
                let n = target.normal;
                let off = liquidcad_core::geometry::dot(sub(*at, target.centroid), n);
                if off.abs() > 1e-3 {
                    return Err(KernelError::Native(format!(
                        "hole point is {:.3} in off face {face}",
                        off.abs()
                    )));
                }
                let dir = [-n[0], -n[1], -n[2]];
                // SAFETY: at and dir hold three doubles each.
                let id = self.id(unsafe {
                    lc_hole(
                        self.0,
                        input.id,
                        at.as_ptr(),
                        dir.as_ptr(),
                        *diameter,
                        *depth,
                    )
                })?;
                self.finish(id, |i, f| match f.tag {
                    TAG_KEPT => kept(f).unwrap_or_else(|| format!("face.{i}")),
                    TAG_HOLE if f.index == 1 => format!("hole{k}.bottom"),
                    TAG_HOLE => format!("hole{k}.wall"),
                    _ => format!("hole{k}.face.{i}"),
                })
            }
            ShapeOp::Cut { tool, pose } => {
                let built = self.build(tool)?;
                let placed = self.transform(built.id, pose, 1.0);
                self.release(built.id);
                let placed = placed?;
                // SAFETY: both ids are live.
                let id = unsafe { lc_cut(self.0, input.id, placed) };
                self.release(placed);
                let id = self.id(id)?;
                let tool_names = built.face_names.clone();
                self.finish(id, |i, f| match f.tag {
                    TAG_KEPT => kept(f).unwrap_or_else(|| format!("face.{i}")),
                    TAG_TOOL => format!(
                        "cut{k}.{}",
                        tool_names
                            .get(f.index as usize)
                            .map(String::as_str)
                            .unwrap_or("face")
                    ),
                    _ => format!("cut{k}.face.{i}"),
                })
            }
        }
    }

    /// A copy of `id` moved by `pose`, then scaled about the origin.
    pub fn transform(&mut self, id: c_int, pose: &Pose, scale: f64) -> Result<c_int, KernelError> {
        let m = pose.matrix_3x4();
        // SAFETY: m holds 12 doubles.
        self.id(unsafe { lc_transform(self.0, id, m.as_ptr(), scale) })
    }

    /// Unit-density integrals about the shape's local origin.
    pub fn integrals(&mut self, id: c_int) -> Result<MassIntegrals, KernelError> {
        let mut out = [0.0f64; 13];
        // SAFETY: out holds 13 doubles.
        if unsafe { lc_mass(self.0, id, out.as_mut_ptr()) } != 0 {
            return Err(self.error());
        }
        let v = out[0];
        let c: Vec3 = [out[1], out[2], out[3]];
        let mut inertia = [[0.0; 3]; 3];
        for (r, row) in inertia.iter_mut().enumerate() {
            for (k, cell) in row.iter_mut().enumerate() {
                *cell = out[4 + 3 * r + k];
            }
        }
        // I = tr(C)·E − C about the centroid, so C = tr(I)/2·E − I; then
        // shift the second moment back to the local origin.
        let tr = inertia[0][0] + inertia[1][1] + inertia[2][2];
        let mut second = [[0.0; 3]; 3];
        for r in 0..3 {
            for k in 0..3 {
                let e = if r == k { tr * 0.5 } else { 0.0 };
                second[r][k] = e - inertia[r][k] + v * c[r] * c[k];
            }
        }
        Ok(MassIntegrals {
            mass: v,
            first: [v * c[0], v * c[1], v * c[2]],
            second,
        })
    }

    pub fn mesh(&mut self, id: c_int, deflection: f64) -> Result<MeshData, KernelError> {
        let mut mesh = LcMesh {
            positions: ptr::null_mut(),
            normals: ptr::null_mut(),
            indices: ptr::null_mut(),
            face_tri_start: ptr::null_mut(),
            vertex_count: 0,
            index_count: 0,
            face_count: 0,
        };
        // SAFETY: mesh is a valid out-pointer; buffers are freed below.
        if unsafe { lc_tessellate(self.0, id, deflection, &mut mesh) } != 0 {
            return Err(self.error());
        }
        // SAFETY: the adapter allocated vertex_count*3 floats, index_count
        // u32s, and face_count + 1 starts.
        let out = unsafe {
            let data = MeshData {
                positions: std::slice::from_raw_parts(mesh.positions, mesh.vertex_count * 3)
                    .to_vec(),
                normals: std::slice::from_raw_parts(mesh.normals, mesh.vertex_count * 3).to_vec(),
                indices: std::slice::from_raw_parts(mesh.indices, mesh.index_count).to_vec(),
                face_tri_start: std::slice::from_raw_parts(
                    mesh.face_tri_start,
                    mesh.face_count + 1,
                )
                .to_vec(),
            };
            lc_mesh_free(&mut mesh);
            data
        };
        Ok(out)
    }

    pub fn polyline(&mut self, id: c_int, edge: usize) -> Vec<Vec3> {
        let mut buf = [0.0f64; 33 * 3];
        // SAFETY: buf holds 33 points.
        let n = unsafe { lc_edge_polyline(self.0, id, edge as c_int, buf.as_mut_ptr(), 33) };
        (0..n.max(0) as usize)
            .map(|k| [buf[3 * k], buf[3 * k + 1], buf[3 * k + 2]])
            .collect()
    }

    pub fn write_step(
        &mut self,
        ids: &[c_int],
        names: &[&str],
        colors: &[[f32; 3]],
        path: &Path,
        product: &str,
    ) -> Result<(), KernelError> {
        let c_names: Vec<CString> = names
            .iter()
            .map(|n| CString::new(*n).map_err(|_| KernelError::Native("name has NUL".into())))
            .collect::<Result<_, _>>()?;
        let ptrs: Vec<*const c_char> = c_names.iter().map(|c| c.as_ptr()).collect();
        let rgb: Vec<f64> = colors.iter().flat_map(|c| c.map(f64::from)).collect();
        let c_path =
            CString::new(path.to_str().ok_or(KernelError::Path)?).map_err(|_| KernelError::Path)?;
        let c_product =
            CString::new(product).map_err(|_| KernelError::Native("product has NUL".into()))?;
        // SAFETY: all buffers hold ids.len() entries and outlive the call.
        let rc = unsafe {
            lc_write_step(
                self.0,
                ids.as_ptr(),
                ptrs.as_ptr(),
                rgb.as_ptr(),
                ids.len(),
                c_path.as_ptr(),
                c_product.as_ptr(),
            )
        };
        if rc != 0 {
            return Err(self.error());
        }
        Ok(())
    }
}

fn sweep_name(i: usize, f: &LcFace, side: &str) -> String {
    let loop_and_segment = |index: c_int| {
        let (l, s) = (index / 10000, index % 10000);
        if l == 0 {
            format!("{side}.{s}")
        } else {
            format!("hole{l}.{side}.{s}")
        }
    };
    match f.tag {
        TAG_BOTTOM => "bottom".into(),
        TAG_TOP => "top".into(),
        TAG_SIDE | TAG_REV_SIDE => loop_and_segment(f.index),
        TAG_REV_START => "start".into(),
        TAG_REV_END => "end".into(),
        _ => format!("face.{i}"),
    }
}
