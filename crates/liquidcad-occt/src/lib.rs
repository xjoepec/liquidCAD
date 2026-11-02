//! Geometry kernel boundary.
//!
//! The engine hands evaluated components to [`Kernel::sync`]. The kernel
//! builds exact solids (OpenCascade when linked, the Rust [`brep`] otherwise),
//! validates them, derives display meshes, and writes STEP. Solids and meshes
//! are cached per stable component id so a rebuild only re-meshes parts whose
//! geometry or colour changed.

pub mod brep;
pub mod step;

#[cfg(occt)]
mod native;

#[cfg(not(occt))]
use brep::BrepSolid;
use liquidcad_core::document::{Component, Mesh};
use liquidcad_core::geometry::{Pose, Shape, ShapeOracle};
use liquidcad_core::ids::ComponentId;
use std::collections::HashMap;
use std::path::Path;

/// Inches to millimetres. The model is in inches, STEP and OCCT in mm.
pub const MM_PER_IN: f64 = 25.4;

#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    #[error("invalid solid {id}: {reason}")]
    InvalidSolid { id: String, reason: String },
    #[error("OpenCascade: {0}")]
    Native(String),
    #[error("path is not valid UTF-8")]
    Path,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Clone, Debug)]
pub struct KernelInfo {
    pub name: String,
    pub native_occt: bool,
}

pub fn kernel_info() -> KernelInfo {
    #[cfg(occt)]
    {
        KernelInfo {
            name: native::version(),
            native_occt: true,
        }
    }
    #[cfg(not(occt))]
    {
        KernelInfo {
            name: "liquidCAD B-rep (OpenCascade not linked)".into(),
            native_occt: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SyncStats {
    pub built: usize,
    pub reused: usize,
    pub removed: usize,
}

#[derive(Clone, PartialEq)]
struct Key {
    shape: Shape,
    pose: Pose,
    color: [f32; 3],
}

struct Entry {
    key: Key,
    mesh: Mesh,
    /// World-frame B-rep in mm, for the Rust STEP writer.
    #[cfg(not(occt))]
    brep: BrepSolid,
    /// Session id of the shape in its local frame.
    #[cfg(occt)]
    native: std::os::raw::c_int,
}

pub struct Kernel {
    entries: HashMap<ComponentId, Entry>,
    order: Vec<ComponentId>,
    #[cfg(occt)]
    session: native::Session,
}

impl Default for Kernel {
    fn default() -> Self {
        Self::new()
    }
}

/// Absolute chord deflection for meshing curved faces, inches.
#[cfg(occt)]
const DEFLECTION_IN: f64 = 0.01;

impl Kernel {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            order: Vec::new(),
            #[cfg(occt)]
            session: native::Session::new().expect("OpenCascade session"),
        }
    }

    /// Brings the kernel in line with `components`. Unchanged solids keep
    /// their B-rep and mesh; changed ones are rebuilt and validated.
    pub fn sync(
        &mut self,
        components: &[Component],
        color_of: impl Fn(&Component) -> [f32; 3],
    ) -> Result<SyncStats, KernelError> {
        let mut stats = SyncStats::default();
        let mut next = HashMap::with_capacity(components.len());
        // Build into `next` first; on failure the kernel keeps its old state.
        let mut fresh: Vec<ComponentId> = Vec::new();
        let result = (|| {
            for component in components {
                let key = Key {
                    shape: component.shape.clone(),
                    pose: component.pose,
                    color: color_of(component),
                };
                if let Some(entry) = self.entries.get(&component.id) {
                    if entry.key == key {
                        stats.reused += 1;
                        continue;
                    }
                }
                let entry = self.build(component, key)?;
                fresh.push(component.id.clone());
                next.insert(component.id.clone(), entry);
                stats.built += 1;
            }
            Ok::<(), KernelError>(())
        })();
        if let Err(e) = result {
            #[cfg(occt)]
            for entry in next.values() {
                self.session.release(entry.native);
            }
            let _ = fresh;
            return Err(e);
        }
        let keep: std::collections::HashSet<&ComponentId> =
            components.iter().map(|c| &c.id).collect();
        let mut old = std::mem::take(&mut self.entries);
        for (id, entry) in old.drain() {
            if keep.contains(&id) && !next.contains_key(&id) {
                next.insert(id, entry);
            } else {
                stats.removed += usize::from(!keep.contains(&id));
                #[cfg(occt)]
                self.session.release(entry.native);
            }
        }
        self.entries = next;
        self.order = components.iter().map(|c| c.id.clone()).collect();
        Ok(stats)
    }

    pub fn meshes(&self) -> Vec<Mesh> {
        self.order
            .iter()
            .filter_map(|id| self.entries.get(id))
            .map(|entry| entry.mesh.clone())
            .collect()
    }

    pub fn mesh(&self, id: &ComponentId) -> Option<&Mesh> {
        self.entries.get(id).map(|e| &e.mesh)
    }

    pub fn solid_count(&self) -> usize {
        self.order.len()
    }

    /// Writes an AP214 STEP file of every solid, in millimetres.
    pub fn write_step(&mut self, path: &Path, product_name: &str) -> Result<u64, KernelError> {
        #[cfg(occt)]
        {
            let mut ids = Vec::with_capacity(self.order.len());
            let mut names = Vec::with_capacity(self.order.len());
            let mut colors = Vec::with_capacity(self.order.len());
            let mut result = Ok(());
            for id in &self.order {
                let Some(e) = self.entries.get(id) else {
                    continue;
                };
                match self.session.transform(e.native, &e.key.pose, MM_PER_IN) {
                    Ok(placed) => {
                        ids.push(placed);
                        names.push(id.as_str());
                        colors.push(e.key.color);
                    }
                    Err(err) => {
                        result = Err(err);
                        break;
                    }
                }
            }
            if result.is_ok() {
                result = self
                    .session
                    .write_step(&ids, &names, &colors, path, product_name);
            }
            for placed in ids {
                self.session.release(placed);
            }
            result?;
        }
        #[cfg(not(occt))]
        {
            let solids: Vec<step::StepSolid> = self
                .order
                .iter()
                .filter_map(|id| self.entries.get(id).map(|e| (id, e)))
                .map(|(id, e)| step::StepSolid {
                    name: id.as_str(),
                    brep: &e.brep,
                    color: e.key.color,
                })
                .collect();
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("model.step");
            let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
            let text = step::write_ap214(
                &solids,
                &step::StepMeta {
                    file_name,
                    timestamp: &timestamp,
                    product_id: "liquidcad-model",
                    product_name,
                },
            );
            std::fs::write(path, text)?;
        }
        Ok(std::fs::metadata(path)?.len())
    }

    #[cfg(not(occt))]
    fn build(&mut self, component: &Component, key: Key) -> Result<Entry, KernelError> {
        let invalid = |reason: String| KernelError::InvalidSolid {
            id: component.id.0.clone(),
            reason,
        };
        let poly = component.shape.polyhedron().ok_or_else(|| {
            invalid(format!(
                "{} needs OpenCascade, which this build does not link",
                component.shape.needs_occt().unwrap_or("this shape")
            ))
        })?;
        let world = poly.transformed(&component.pose);
        let brep = BrepSolid::from_polyhedron(&world.scaled(MM_PER_IN));
        brep.validate().map_err(|e| invalid(e.to_string()))?;
        let d = brep.display(MM_PER_IN);
        let mesh = Mesh {
            component_id: component.id.clone(),
            positions: d.positions,
            normals: d.normals,
            indices: d.indices,
            edges: d.wire,
            color: key.color,
            faces: d.faces,
            edge_info: d.edges,
        };
        Ok(Entry { key, mesh, brep })
    }

    #[cfg(occt)]
    fn build(&mut self, component: &Component, key: Key) -> Result<Entry, KernelError> {
        let invalid = |reason: String| KernelError::InvalidSolid {
            id: component.id.0.clone(),
            reason,
        };
        let built = self
            .session
            .build(&component.shape)
            .map_err(|e| invalid(e.to_string()))?;
        let mesh = self.posed_mesh(component, &built, key.color).map_err(|e| {
            self.session.release(built.id);
            invalid(e.to_string())
        })?;
        Ok(Entry {
            key,
            mesh,
            native: built.id,
        })
    }

    #[cfg(occt)]
    fn posed_mesh(
        &mut self,
        component: &Component,
        built: &native::Built,
        color: [f32; 3],
    ) -> Result<Mesh, KernelError> {
        use liquidcad_core::document::{CurveKind, EdgeInfo, FaceInfo, SurfaceKind};
        let pose = component.pose;
        let data = self.session.mesh(built.id, DEFLECTION_IN)?;
        let mut positions = Vec::with_capacity(data.positions.len());
        for p in data.positions.chunks(3) {
            let w = pose.apply([f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]);
            positions.extend(w.map(|v| v as f32));
        }
        let mut normals = Vec::with_capacity(data.normals.len());
        for n in data.normals.chunks(3) {
            let w = pose.rotate([f64::from(n[0]), f64::from(n[1]), f64::from(n[2])]);
            normals.extend(w.map(|v| v as f32));
        }
        let faces = built
            .faces
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let start = data.face_tri_start.get(i).copied().unwrap_or(0);
                let end = data.face_tri_start.get(i + 1).copied().unwrap_or(start);
                FaceInfo {
                    id: built.face_names[i].clone(),
                    kind: match f.surface {
                        0 => SurfaceKind::Plane,
                        1 => SurfaceKind::Cylinder,
                        2 => SurfaceKind::Cone,
                        3 => SurfaceKind::Sphere,
                        4 => SurfaceKind::Torus,
                        _ => SurfaceKind::Other,
                    },
                    area_in2: f.area,
                    centroid: pose.apply(f.centroid),
                    normal: pose.rotate(f.normal),
                    tri_start: start,
                    tri_count: end - start,
                }
            })
            .collect();
        let mut wire = Vec::new();
        let mut edge_info = Vec::with_capacity(built.edges.len());
        for (i, e) in built.edges.iter().enumerate() {
            let points: Vec<[f64; 3]> = self
                .session
                .polyline(built.id, i)
                .into_iter()
                .map(|p| pose.apply(p))
                .collect();
            for w in points.windows(2) {
                for p in w {
                    wire.extend(p.map(|v| v as f32));
                }
            }
            edge_info.push(EdgeInfo {
                id: built.edge_names[i].clone(),
                kind: match e.curve {
                    0 => CurveKind::Line,
                    1 => CurveKind::Circle,
                    2 => CurveKind::Arc,
                    _ => CurveKind::Other,
                },
                length_in: e.length,
                polyline: points.iter().flat_map(|p| p.map(|v| v as f32)).collect(),
            });
        }
        Ok(Mesh {
            component_id: component.id.clone(),
            positions,
            normals,
            indices: data.indices,
            edges: wire,
            color,
            faces,
            edge_info,
        })
    }
}

/// Mass integrals for shapes the core cannot integrate itself. Backed by
/// OpenCascade when linked; otherwise only polyhedra.
pub fn oracle() -> Box<dyn ShapeOracle> {
    #[cfg(occt)]
    {
        Box::new(NativeOracle::default())
    }
    #[cfg(not(occt))]
    {
        Box::new(liquidcad_core::geometry::PolyhedralOracle)
    }
}

#[cfg(occt)]
#[derive(Default)]
struct NativeOracle {
    session: std::cell::RefCell<Option<native::Session>>,
    cache: std::cell::RefCell<HashMap<String, liquidcad_core::geometry::MassIntegrals>>,
}

#[cfg(occt)]
impl ShapeOracle for NativeOracle {
    fn integrals(&self, shape: &Shape) -> Result<liquidcad_core::geometry::MassIntegrals, String> {
        if let Some(exact) = shape.volume_integrals() {
            return Ok(exact);
        }
        let key = serde_json::to_string(shape).map_err(|e| e.to_string())?;
        if let Some(hit) = self.cache.borrow().get(&key) {
            return Ok(*hit);
        }
        let mut slot = self.session.borrow_mut();
        if slot.is_none() {
            *slot = Some(native::Session::new().map_err(|e| e.to_string())?);
        }
        let session = slot.as_mut().expect("session");
        let built = session.build(shape).map_err(|e| e.to_string())?;
        let result = session.integrals(built.id).map_err(|e| e.to_string());
        session.release(built.id);
        let integrals = result?;
        self.cache.borrow_mut().insert(key, integrals);
        Ok(integrals)
    }

    fn native(&self) -> bool {
        true
    }

    fn face(
        &self,
        shape: &Shape,
        name: &str,
    ) -> Result<liquidcad_core::geometry::FaceFrame, String> {
        if let Some(poly) = shape.polyhedron() {
            return poly
                .face_frame(name)
                .ok_or_else(|| format!("face {name} does not exist on this part"));
        }
        self.with_built(shape, |_, built| {
            let i = built
                .face_names
                .iter()
                .position(|n| n == name)
                .ok_or_else(|| format!("face {name} does not exist on this part"))?;
            let f = &built.faces[i];
            if f.surface != 0 {
                return Err(format!("face {name} is curved; pick a flat face"));
            }
            Ok(liquidcad_core::geometry::FaceFrame {
                centroid: f.centroid,
                normal: f.normal,
                area: f.area,
            })
        })
    }

    fn edge(&self, shape: &Shape, name: &str) -> Result<([f64; 3], [f64; 3]), String> {
        if let Some(poly) = shape.polyhedron() {
            return poly
                .edge_list()
                .into_iter()
                .find(|e| e.name == name)
                .map(|e| (poly.vertices[e.a], poly.vertices[e.b]))
                .ok_or_else(|| format!("edge {name} does not exist on this part"));
        }
        self.with_built(shape, |session, built| {
            let i = built
                .edge_names
                .iter()
                .position(|n| n == name)
                .ok_or_else(|| format!("edge {name} does not exist on this part"))?;
            if built.edges[i].curve != 0 {
                return Err(format!("edge {name} is curved; pick a straight edge"));
            }
            let pts = session.polyline(built.id, i);
            match (pts.first(), pts.last()) {
                (Some(a), Some(b)) => Ok((*a, *b)),
                _ => Err(format!("edge {name} has no geometry")),
            }
        })
    }
}

#[cfg(occt)]
impl NativeOracle {
    fn with_built<T>(
        &self,
        shape: &Shape,
        f: impl FnOnce(&mut native::Session, &native::Built) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut slot = self.session.borrow_mut();
        if slot.is_none() {
            *slot = Some(native::Session::new().map_err(|e| e.to_string())?);
        }
        let session = slot.as_mut().expect("session");
        let built = session.build(shape).map_err(|e| e.to_string())?;
        let out = f(session, &built);
        session.release(built.id);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use liquidcad_core::{rebuild, templates::modular_dock};

    fn grey(_: &Component) -> [f32; 3] {
        [0.5, 0.5, 0.5]
    }

    #[test]
    fn sync_builds_then_reuses_unchanged_solids() {
        let mut project = modular_dock();
        let doc = rebuild(&project, 1).unwrap();
        let mut kernel = Kernel::new();
        let first = kernel.sync(&doc.components, grey).unwrap();
        assert_eq!(first.built, doc.components.len());
        assert_eq!(kernel.meshes().len(), doc.components.len());

        project.set_parameter("deck_thickness_in", 1.5).unwrap();
        let doc2 = rebuild(&project, 2).unwrap();
        let second = kernel.sync(&doc2.components, grey).unwrap();
        let floats = doc2
            .components
            .iter()
            .filter(|c| c.kind == liquidcad_core::ComponentKind::FloatModule)
            .count();
        assert!(second.reused >= floats, "{second:?}");
        assert!(second.built > 0);
        assert_eq!(second.built + second.reused, doc2.components.len());
    }

    #[test]
    fn step_export_is_well_formed_and_complete() {
        let doc = rebuild(&modular_dock(), 1).unwrap();
        let mut kernel = Kernel::new();
        kernel.sync(&doc.components, grey).unwrap();
        let dir = std::env::temp_dir().join(format!("liquidcad-kernel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dock.step");
        let bytes = kernel.write_step(&path, "Modular floating dock").unwrap();
        assert!(bytes > 10_000);
        let text = std::fs::read_to_string(&path).unwrap();
        let parsed = step::check::parse(&text).unwrap();
        let solids = parsed
            .values()
            .filter(|i| i.keywords.iter().any(|k| k == "MANIFOLD_SOLID_BREP"))
            .count();
        assert_eq!(solids, doc.components.len());
        assert!(text.contains("'float.r0.c0'"));
        assert!(text.contains("'frame.stringer.stbd'"));
    }

    #[test]
    fn meshes_are_in_inches() {
        let doc = rebuild(&modular_dock(), 1).unwrap();
        let mut kernel = Kernel::new();
        kernel.sync(&doc.components, grey).unwrap();
        let max = kernel
            .meshes()
            .iter()
            .flat_map(|m| m.positions.iter().copied())
            .fold(f32::MIN, f32::max);
        assert!((max - 192.0).abs() < 3.0, "max coordinate {max}");
    }

    #[test]
    fn faces_are_named_and_cover_the_mesh() {
        let doc = rebuild(&modular_dock(), 1).unwrap();
        let mut kernel = Kernel::new();
        kernel.sync(&doc.components, grey).unwrap();
        let board = kernel
            .mesh(&ComponentId::new("deck.board.0"))
            .unwrap()
            .clone();
        let mut names: Vec<&str> = board.faces.iter().map(|f| f.id.as_str()).collect();
        names.sort();
        assert_eq!(names, ["x+", "x-", "y+", "y-", "z+", "z-"]);
        let tris: u32 = board.faces.iter().map(|f| f.tri_count).sum();
        assert_eq!(tris as usize * 3, board.indices.len());
        let top = board.faces.iter().find(|f| f.id == "z+").unwrap();
        assert!((top.area_in2 - 96.0 * 5.5).abs() < 1e-6, "{}", top.area_in2);
        assert!((top.normal[2] - 1.0).abs() < 1e-9);
        assert_eq!(board.edge_info.len(), 12);
        let long = board.edge_info.iter().find(|e| e.id == "y-|z+").unwrap();
        assert!((long.length_in - 96.0).abs() < 1e-6);
    }

    #[cfg(occt)]
    mod native_ops {
        use super::*;
        use liquidcad_core::geometry::{PathVertex, Profile, ShapeOp};
        use std::f64::consts::PI;

        fn volume(shape: &Shape) -> f64 {
            oracle().integrals(shape).unwrap().mass
        }

        fn cube() -> Shape {
            Shape::cuboid([4.0, 4.0, 4.0])
        }

        #[test]
        fn fillet_removes_the_analytic_volume() {
            let filleted = Shape::Modified {
                base: Box::new(cube()),
                ops: vec![ShapeOp::Fillet {
                    edges: vec!["x+|y+".into()],
                    radius: 1.0,
                }],
            };
            let removed = 64.0 - volume(&filleted);
            assert!((removed - (1.0 - PI / 4.0) * 4.0).abs() < 1e-6, "{removed}");
            let mut s = native::Session::new().unwrap();
            let built = s.build(&filleted).unwrap();
            assert!(built.face_names.iter().any(|n| n == "fillet(x+|y+)"));
            assert!(built.face_names.iter().any(|n| n == "z+"));
        }

        #[test]
        fn through_hole_removes_a_cylinder_and_names_its_wall() {
            let plate = Shape::Modified {
                base: Box::new(Shape::cuboid([4.0, 4.0, 1.0])),
                ops: vec![ShapeOp::Hole {
                    face: "z+".into(),
                    at: [2.0, 2.0, 1.0],
                    diameter: 1.0,
                    depth: 0.0,
                }],
            };
            assert!((volume(&plate) - (16.0 - PI * 0.25)).abs() < 1e-6);
            let mut s = native::Session::new().unwrap();
            let built = s.build(&plate).unwrap();
            assert!(built.face_names.iter().any(|n| n == "hole1.wall"));
            let off_face = Shape::Modified {
                base: Box::new(Shape::cuboid([4.0, 4.0, 1.0])),
                ops: vec![ShapeOp::Hole {
                    face: "z+".into(),
                    at: [2.0, 2.0, 0.5],
                    diameter: 1.0,
                    depth: 0.0,
                }],
            };
            assert!(oracle()
                .integrals(&off_face)
                .unwrap_err()
                .contains("off face z+"));
        }

        #[test]
        fn arcs_and_profile_holes_are_exact() {
            let washer = Shape::Prism {
                profile: Profile {
                    outer: vec![
                        PathVertex {
                            x: -2.0,
                            y: 0.0,
                            bulge: 1.0,
                        },
                        PathVertex {
                            x: 2.0,
                            y: 0.0,
                            bulge: 1.0,
                        },
                    ],
                    holes: vec![vec![
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
                    ]],
                },
                depth: 0.5,
            };
            assert!((volume(&washer) - PI * (4.0 - 1.0) * 0.5).abs() < 1e-6);
            let cylinder = Shape::Revolve {
                profile: Profile::rectangle(1.0, 3.0),
                angle_deg: 360.0,
            };
            assert!((volume(&cylinder) - PI * 3.0).abs() < 1e-6);
        }

        #[test]
        fn native_and_rust_name_box_faces_alike() {
            let shape = Shape::cuboid([2.0, 3.0, 5.0]);
            let rust = brep::BrepSolid::from_polyhedron(&shape.polyhedron().unwrap()).display(1.0);
            let mut s = native::Session::new().unwrap();
            let built = s.build(&shape).unwrap();
            for face in &rust.faces {
                let i = built.face_names.iter().position(|n| *n == face.id).unwrap();
                assert!(
                    (built.faces[i].area - face.area_in2).abs() < 1e-9,
                    "{}",
                    face.id
                );
            }
            let mut a: Vec<_> = rust.edges.iter().map(|e| e.id.clone()).collect();
            let mut b = built.edge_names.clone();
            a.sort();
            b.sort();
            assert_eq!(a, b);
        }

        #[test]
        fn modified_parts_export_to_step() {
            let mut doc = rebuild(&modular_dock(), 1).unwrap();
            let board = doc
                .components
                .iter_mut()
                .find(|c| c.id.as_str() == "deck.board.0")
                .unwrap();
            board.shape = Shape::Modified {
                base: Box::new(board.shape.clone()),
                ops: vec![ShapeOp::Fillet {
                    edges: vec!["y-|z+".into(), "y+|z+".into()],
                    radius: 0.25,
                }],
            };
            let mut kernel = Kernel::new();
            kernel.sync(&doc.components, grey).unwrap();
            let mesh = kernel.mesh(&ComponentId::new("deck.board.0")).unwrap();
            assert!(mesh.faces.iter().any(|f| f.id == "fillet(y-|z+)"
                && f.kind == liquidcad_core::document::SurfaceKind::Cylinder));
            let dir = std::env::temp_dir().join(format!("liquidcad-mod-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("mod.step");
            kernel.write_step(&path, "dock").unwrap();
            let text = std::fs::read_to_string(&path).unwrap();
            step::check::parse(&text).unwrap();
            assert!(text.contains("CYLINDRICAL_SURFACE"));
        }
    }

    #[test]
    fn kernel_reports_its_backend() {
        let info = kernel_info();
        assert!(!info.name.is_empty());
        assert_eq!(info.native_occt, cfg!(occt));
    }
}
