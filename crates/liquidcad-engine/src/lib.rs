//! CAD session: owns the project, the incremental evaluator, the geometry
//! kernel, undo/redo, and the SQLite history. Every edit is transactional:
//! the candidate project is evaluated and meshed first, and only a fully
//! successful rebuild replaces the current state.

use liquidcad_core::diagnostics::Severity;
use liquidcad_core::document::{ComponentKind, Document};
use liquidcad_core::evaluate::RebuildReport;
use liquidcad_core::feature::{Feature, FeatureKind, FeatureSpec, SketchPlane};
use liquidcad_core::ids::{ComponentId, FeatureId};
use liquidcad_core::mates::Mate;
use liquidcad_core::project::Placement;
use liquidcad_core::{Evaluator, Project};
use liquidcad_occt::{Kernel, SyncStats};
use liquidcad_protocol::{
    ExportFormat, FeatureDraft, KernelStatus, PreviewResult, Request, Response, SelectionInfo,
    SessionState, Value, PROTOCOL_VERSION,
};
use liquidcad_store::Store;
use std::path::{Path, PathBuf};

const UNDO_LIMIT: usize = 200;
/// Placement moves snap to this grid (inches).
pub const MOVE_SNAP_IN: f64 = 0.5;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Core(#[from] liquidcad_core::Error),
    #[error(transparent)]
    Store(#[from] liquidcad_store::StoreError),
    #[error(transparent)]
    Kernel(#[from] liquidcad_occt::KernelError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Session(String),
}

impl EngineError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Core(liquidcad_core::Error::Validation(_)) => "validation",
            Self::Core(_) => "model",
            Self::Store(_) => "store",
            Self::Kernel(_) => "kernel",
            Self::Io(_) => "io",
            Self::Session(_) => "session",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct EngineConfig {
    /// Directory for `liquidcad.sqlite`. `None` disables history.
    pub data_dir: Option<PathBuf>,
}

pub struct Engine {
    project: Option<Project>,
    project_path: Option<PathBuf>,
    document: Option<Document>,
    evaluator: Evaluator,
    kernel: Kernel,
    store: Option<Store>,
    undo: Vec<(Project, String)>,
    redo: Vec<(Project, String)>,
    selected: Option<ComponentId>,
    revision: u64,
    dirty: bool,
    last_report: RebuildReport,
    last_sync: SyncStats,
}

impl Engine {
    pub fn new(config: EngineConfig) -> Result<Self, EngineError> {
        let store = match &config.data_dir {
            Some(dir) => Some(Store::open(&dir.join("liquidcad.sqlite"))?),
            None => None,
        };
        Ok(Self {
            project: None,
            project_path: None,
            document: None,
            evaluator: Evaluator::with_oracle(liquidcad_occt::oracle()),
            kernel: Kernel::new(),
            store,
            undo: Vec::new(),
            redo: Vec::new(),
            selected: None,
            revision: 0,
            dirty: false,
            last_report: RebuildReport::default(),
            last_sync: SyncStats::default(),
        })
    }

    pub fn status() -> KernelStatus {
        let info = liquidcad_occt::kernel_info();
        KernelStatus {
            kernel: info.name,
            native_occt: info.native_occt,
            protocol: PROTOCOL_VERSION,
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }

    pub fn handle(&mut self, id: u64, request: Request) -> Response {
        match self.dispatch(request) {
            Ok(value) => Response::ok(id, value),
            Err(err) => Response::err(id, err.code(), err.to_string()),
        }
    }

    fn session(&self) -> Result<Value, EngineError> {
        Ok(Value::Session(Box::new(self.state()?)))
    }

    fn dispatch(&mut self, request: Request) -> Result<Value, EngineError> {
        match request {
            Request::Ping => Ok(Value::Pong(Self::status())),
            Request::Open { path } => {
                self.open(Path::new(&path))?;
                self.session()
            }
            Request::Document => self.session(),
            Request::SetParameter { name, value } => {
                self.set_parameter(&name, value)?;
                self.session()
            }
            Request::SetMaterial { feature, material } => {
                self.set_material(&feature, &material)?;
                self.session()
            }
            Request::SetSuppressed {
                feature,
                suppressed,
            } => {
                self.set_suppressed(&feature, suppressed)?;
                self.session()
            }
            Request::MovePart { id, offset } => {
                self.move_part(&id, offset)?;
                self.session()
            }
            Request::Select { id } => {
                self.select(id.as_deref())?;
                Ok(Value::Selection(self.selection_info()?.map(Box::new)))
            }
            Request::Undo => {
                self.undo()?;
                self.session()
            }
            Request::Redo => {
                self.redo()?;
                self.session()
            }
            Request::History { limit } => {
                Ok(Value::History(self.history(limit.unwrap_or(50) as usize)?))
            }
            Request::Restore { revision } => {
                self.restore(revision)?;
                self.session()
            }
            Request::Drawing => Ok(Value::Drawing(Box::new(liquidcad_mfg::drawing(
                self.document()?,
            )))),
            Request::CutPlan => Ok(Value::CutPlan(liquidcad_mfg::cutlist::plan(
                self.document()?,
            ))),
            Request::Export { format, path } => {
                let bytes = self.export(format, Path::new(&path))?;
                Ok(Value::Exported {
                    format,
                    path,
                    bytes,
                })
            }
            Request::Save { path } => {
                let saved = self.save(path.as_deref().map(Path::new))?;
                Ok(Value::Saved {
                    path: saved.display().to_string(),
                })
            }
            Request::AddFeature { draft } => {
                self.add_feature(draft)?;
                self.session()
            }
            Request::EditFeature { id, draft } => {
                self.edit_feature(&id, draft)?;
                self.session()
            }
            Request::DeleteFeature { id } => {
                self.delete_feature(&id)?;
                self.session()
            }
            Request::PreviewFeature { draft } => {
                Ok(Value::Preview(Box::new(self.preview_feature(draft)?)))
            }
            Request::AddMate { mate } => {
                self.add_mate(mate)?;
                self.session()
            }
            Request::DeleteMate { id } => {
                self.delete_mate(&id)?;
                self.session()
            }
            Request::Simulate { params } => {
                Ok(Value::Simulation(Box::new(self.simulate(&params)?)))
            }
        }
    }

    /// Turns a draft into a feature: id, parents (everything it reads from),
    /// inputs, and material.
    fn draft_feature(&self, id: FeatureId, draft: FeatureDraft) -> Result<Feature, EngineError> {
        let document = self.document()?;
        let owner = |component: &str| -> Result<FeatureId, EngineError> {
            document
                .components
                .iter()
                .find(|c| c.id.0 == component)
                .map(|c| c.feature_id.clone())
                .ok_or_else(|| invalid(format!("part {component} does not exist")))
        };
        let project = self.project()?;
        // Earlier user features that rewrite `component`, so a new modifier
        // stacks on top of them.
        let modifiers_of = |component: &str| -> Vec<FeatureId> {
            project
                .features
                .iter()
                .filter(|f| f.id != id)
                .filter(|f| match &f.spec {
                    Some(FeatureSpec::Modify(m)) => m.target == component,
                    Some(FeatureSpec::Extrude(e)) => e.cut_into.as_deref() == Some(component),
                    None => false,
                })
                .map(|f| f.id.clone())
                .collect()
        };
        let mut parents = Vec::new();
        let mut inputs = Vec::new();
        let (kind, material) = match &draft.spec {
            FeatureSpec::Extrude(spec) => {
                if let SketchPlane::Face { component, .. } = &spec.plane {
                    parents.push(owner(component)?);
                    parents.extend(modifiers_of(component));
                }
                if let Some(target) = &spec.cut_into {
                    parents.push(owner(target)?);
                    parents.extend(modifiers_of(target));
                }
                if let Some(p) = &spec.depth_parameter {
                    inputs.push(p.clone());
                }
                (
                    FeatureKind::Extrude,
                    Some(draft.material.clone().unwrap_or_else(|| "alum_6061".into())),
                )
            }
            FeatureSpec::Modify(spec) => {
                parents.push(owner(&spec.target)?);
                parents.extend(modifiers_of(&spec.target));
                (FeatureKind::Modify, None)
            }
        };
        if parents.is_empty() {
            parents.push(FeatureId::new("feat.parameters"));
        }
        parents.dedup();
        let name = draft.name.trim();
        if name.is_empty() || name.len() > 80 {
            return Err(invalid("feature name must be 1 to 80 characters".into()));
        }
        Ok(Feature {
            id,
            name: name.to_string(),
            parents,
            kind,
            inputs,
            material,
            suppressed: false,
            spec: Some(draft.spec),
        })
    }

    fn next_feature_id(&self, kind: &str) -> Result<FeatureId, EngineError> {
        let project = self.project()?;
        let n = (1..)
            .find(|n| {
                let id = format!("feat.{kind}.{n}");
                !project.features.iter().any(|f| f.id.0 == id)
            })
            .unwrap_or(1);
        Ok(FeatureId::new(format!("feat.{kind}.{n}")))
    }

    /// Rejects a candidate project whose new feature fails to build, with
    /// the reason, before it can reach the undo stack.
    fn check_feature(&mut self, next: &Project, id: &FeatureId) -> Result<(), EngineError> {
        let (document, _) = self.evaluator.rebuild(next, self.revision)?;
        if let Some(d) = document.diagnostics.iter().find(|d| {
            d.severity == Severity::Error && d.related_ids.iter().any(|r| r == id.as_str())
        }) {
            return Err(invalid(d.message.clone()));
        }
        Ok(())
    }

    pub fn add_feature(&mut self, draft: FeatureDraft) -> Result<FeatureId, EngineError> {
        let kind = match draft.spec {
            FeatureSpec::Extrude(_) => "extrude",
            FeatureSpec::Modify(_) => "modify",
        };
        let id = self.next_feature_id(kind)?;
        let feature = self.draft_feature(id.clone(), draft)?;
        let label = format!("add {}", feature.name);
        let mut next = self.project()?.clone();
        next.add_feature(feature)?;
        self.check_feature(&next, &id)?;
        self.apply(label, move |p| {
            *p = next;
            Ok(())
        })?;
        Ok(id)
    }

    pub fn edit_feature(&mut self, id: &str, draft: FeatureDraft) -> Result<(), EngineError> {
        let fid = FeatureId::new(id);
        let feature = self.draft_feature(fid.clone(), draft)?;
        let mut next = self.project()?.clone();
        let slot = next
            .features
            .iter_mut()
            .find(|f| f.id == fid)
            .ok_or_else(|| liquidcad_core::Error::UnknownFeature(id.to_string()))?;
        if slot.spec.is_none() {
            return Err(invalid(format!(
                "{} is generated by the template; edit its parameters instead",
                slot.name
            )));
        }
        let label = format!("edit {}", feature.name);
        slot.name = feature.name;
        slot.spec = feature.spec;
        slot.inputs = feature.inputs;
        slot.parents = feature.parents;
        if feature.material.is_some() && slot.kind == FeatureKind::Extrude {
            slot.material = slot.material.clone().or(feature.material);
        }
        next.validate()?;
        self.check_feature(&next, &fid)?;
        self.apply(label, move |p| {
            *p = next;
            Ok(())
        })
    }

    pub fn delete_feature(&mut self, id: &str) -> Result<(), EngineError> {
        let name = self
            .project()?
            .features
            .iter()
            .find(|f| f.id.0 == id)
            .map(|f| f.name.clone())
            .unwrap_or_else(|| id.to_string());
        self.apply(format!("delete {name}"), |p| {
            p.remove_feature(id)?;
            Ok(())
        })
    }

    /// Builds the draft in a scratch session and returns the parts it adds
    /// or changes. Nothing is committed and the live kernel is untouched.
    pub fn preview_feature(&mut self, draft: FeatureDraft) -> Result<PreviewResult, EngineError> {
        let kind = match draft.spec {
            FeatureSpec::Extrude(_) => "extrude",
            FeatureSpec::Modify(_) => "modify",
        };
        let id = self.next_feature_id(kind)?;
        let feature = self.draft_feature(id.clone(), draft)?;
        let mut next = self.project()?.clone();
        next.add_feature(feature)?;
        let mut scratch = Evaluator::with_oracle(liquidcad_occt::oracle());
        let (document, _) = scratch.rebuild(&next, self.revision)?;
        let before = self.document()?;
        let touched: Vec<&liquidcad_core::Component> = document
            .components
            .iter()
            .filter(|c| {
                c.feature_id == id
                    || before
                        .components
                        .iter()
                        .find(|b| b.id == c.id)
                        .is_some_and(|b| b.shape != c.shape)
            })
            .collect();
        let diagnostics: Vec<_> = document
            .diagnostics
            .iter()
            .filter(|d| d.related_ids.iter().any(|r| r == id.as_str()))
            .cloned()
            .collect();
        let mut kernel = Kernel::new();
        let parts: Vec<liquidcad_core::Component> = touched.iter().map(|c| (*c).clone()).collect();
        let meshes = match kernel.sync(&parts, |_| [0.95, 0.64, 0.23]) {
            Ok(_) => kernel.meshes(),
            Err(_) => Vec::new(),
        };
        let mass = touched
            .iter()
            .find(|c| c.feature_id == id)
            .or(touched.first())
            .map(|c| c.mass);
        Ok(PreviewResult {
            meshes,
            diagnostics,
            mass,
        })
    }

    pub fn add_mate(&mut self, mate: Mate) -> Result<(), EngineError> {
        let label = format!("add mate {}", mate.name);
        let id = mate.id.clone();
        let mut next = self.project()?.clone();
        next.add_mate(mate)?;
        let (document, _) = self.evaluator.rebuild(&next, self.revision)?;
        if let Some(d) = document
            .diagnostics
            .iter()
            .find(|d| d.code.starts_with("MATE-") && d.severity == Severity::Error)
        {
            return Err(invalid(d.message.clone()));
        }
        if !document.mates.iter().any(|m| m.id == id) {
            return Err(invalid("the mate could not be solved".into()));
        }
        self.apply(label, move |p| {
            *p = next;
            Ok(())
        })
    }

    pub fn delete_mate(&mut self, id: &str) -> Result<(), EngineError> {
        self.apply(format!("delete mate {id}"), |p| {
            p.remove_mate(id)?;
            Ok(())
        })
    }

    /// Runs the wave/motion screening on the current design.
    pub fn simulate(
        &self,
        params: &liquidcad_sim::SimParams,
    ) -> Result<liquidcad_sim::SimResult, EngineError> {
        let document = self.document()?;
        let e = &document.engineering;
        let floats = document
            .components
            .iter()
            .filter(|c| c.kind == ComponentKind::FloatModule)
            .map(|c| liquidcad_sim::FloatBox {
                min: c.bounds.origin,
                size: c.bounds.size,
            })
            .collect();
        let width = self.project()?.parameter_value("width_in").unwrap_or(0.0);
        let length = self.project()?.parameter_value("length_in").unwrap_or(0.0);
        let body = liquidcad_sim::Body {
            mass: e.floating,
            floats,
            deck_top_in: e.deck_top_in,
            deck_corners: [[0.0, 0.0], [width, 0.0], [width, length], [0.0, length]],
        };
        liquidcad_sim::simulate(&body, params).map_err(invalid)
    }

    pub fn open(&mut self, path: &Path) -> Result<(), EngineError> {
        let project = Project::load_path(path)?;
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.load(project, Some(path))
    }

    /// Replaces the session with `project`. Nothing changes if it fails to build.
    pub fn load(&mut self, project: Project, path: Option<PathBuf>) -> Result<(), EngineError> {
        let mut evaluator = Evaluator::with_oracle(liquidcad_occt::oracle());
        let mut kernel = Kernel::new();
        let revision = self.revision + 1;
        let (document, report, sync) = build(&mut evaluator, &mut kernel, &project, revision)?;
        if let Some(store) = &self.store {
            store.touch_project(
                &project,
                &path
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
            )?;
            let latest = store.history(&project.id, 1)?;
            let changed = match latest.first() {
                Some(rev) => {
                    store.snapshot(&project.id, rev.id)?.to_json()? != project.to_json()?
                }
                None => true,
            };
            if changed {
                store.commit(
                    &project,
                    "open",
                    document.components.len(),
                    document.engineering.dead_load_lb,
                )?;
            }
        }
        self.evaluator = evaluator;
        self.kernel = kernel;
        self.project = Some(project);
        self.project_path = path;
        self.document = Some(document);
        self.revision = revision;
        self.last_report = report;
        self.last_sync = sync;
        self.undo.clear();
        self.redo.clear();
        self.selected = None;
        self.dirty = false;
        Ok(())
    }

    fn apply(
        &mut self,
        note: String,
        edit: impl FnOnce(&mut Project) -> Result<(), EngineError>,
    ) -> Result<(), EngineError> {
        let current = self.project()?.clone();
        let mut next = current.clone();
        edit(&mut next)?;
        self.commit_state(next, note.clone())?;
        self.undo.push((current, note));
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        Ok(())
    }

    /// Rebuilds with `next`; on success makes it current and records history.
    fn commit_state(&mut self, next: Project, note: String) -> Result<(), EngineError> {
        let revision = self.revision + 1;
        let (document, report, sync) =
            build(&mut self.evaluator, &mut self.kernel, &next, revision)?;
        if let Some(store) = &self.store {
            store.commit(
                &next,
                &note,
                document.components.len(),
                document.engineering.dead_load_lb,
            )?;
        }
        if let Some(selected) = &self.selected {
            if !document.components.iter().any(|c| &c.id == selected) {
                self.selected = None;
            }
        }
        self.project = Some(next);
        self.document = Some(document);
        self.revision = revision;
        self.last_report = report;
        self.last_sync = sync;
        self.dirty = true;
        Ok(())
    }

    pub fn set_parameter(&mut self, name: &str, value: f64) -> Result<(), EngineError> {
        let label = self
            .project()?
            .parameters
            .get(name)
            .map(|p| p.label.clone())
            .unwrap_or_else(|| name.to_string());
        self.apply(format!("{label} = {value}"), |p| {
            p.set_parameter(name, value)?;
            Ok(())
        })
    }

    pub fn set_material(&mut self, feature: &str, material: &str) -> Result<(), EngineError> {
        self.apply(format!("{feature} material = {material}"), |p| {
            p.set_feature_material(feature, material)?;
            Ok(())
        })
    }

    pub fn set_suppressed(&mut self, feature: &str, suppressed: bool) -> Result<(), EngineError> {
        let verb = if suppressed { "suppress" } else { "unsuppress" };
        self.apply(format!("{verb} {feature}"), |p| {
            p.set_suppressed(feature, suppressed)?;
            Ok(())
        })
    }

    /// Moves a movable part by `offset` from its generated position. The
    /// offset snaps to [`MOVE_SNAP_IN`], stays on the deck plane, and must
    /// keep the part on the deck.
    pub fn move_part(&mut self, id: &str, offset: [f64; 3]) -> Result<(), EngineError> {
        let document = self.document()?;
        let component = document.component(&ComponentId::new(id))?;
        if !component.kind.movable() {
            return Err(EngineError::Core(liquidcad_core::Error::Validation(
                format!(
                    "{} is placed by its feature and cannot be moved",
                    component.name
                ),
            )));
        }
        if offset.iter().any(|v| !v.is_finite()) {
            return Err(EngineError::Core(liquidcad_core::Error::Validation(
                "offset must be finite".into(),
            )));
        }
        let snapped = [snap(offset[0]), snap(offset[1]), 0.0];
        let width = self.project()?.parameter_value("width_in")?;
        let length = self.project()?.parameter_value("length_in")?;
        let nominal = component.bounds.translated([
            -component.placement[0],
            -component.placement[1],
            -component.placement[2],
        ]);
        let moved = nominal.translated(snapped);
        if moved.origin[0] < -1e-9
            || moved.origin[1] < -1e-9
            || moved.max()[0] > width + 1e-9
            || moved.max()[1] > length + 1e-9
        {
            return Err(EngineError::Core(liquidcad_core::Error::Validation(
                format!(
                    "{} must stay on the {width} x {length} in deck",
                    component.name
                ),
            )));
        }
        let name = component.name.clone();
        self.apply(format!("move {name}"), |p| {
            if snapped == [0.0; 3] {
                p.placements.remove(id);
            } else {
                p.placements
                    .insert(id.to_string(), Placement { offset: snapped });
            }
            Ok(())
        })
    }

    pub fn select(&mut self, id: Option<&str>) -> Result<(), EngineError> {
        match id {
            None => self.selected = None,
            Some(id) => {
                let id = ComponentId::new(id);
                self.document()?.component(&id)?;
                self.selected = Some(id);
            }
        }
        Ok(())
    }

    pub fn undo(&mut self) -> Result<(), EngineError> {
        let (previous, note) = self
            .undo
            .pop()
            .ok_or_else(|| EngineError::Session("nothing to undo".into()))?;
        let current = self.project()?.clone();
        if let Err(err) = self.commit_state(previous.clone(), format!("undo {note}")) {
            self.undo.push((previous, note));
            return Err(err);
        }
        self.redo.push((current, note));
        Ok(())
    }

    pub fn redo(&mut self) -> Result<(), EngineError> {
        let (next, note) = self
            .redo
            .pop()
            .ok_or_else(|| EngineError::Session("nothing to redo".into()))?;
        let current = self.project()?.clone();
        if let Err(err) = self.commit_state(next.clone(), format!("redo {note}")) {
            self.redo.push((next, note));
            return Err(err);
        }
        self.undo.push((current, note));
        Ok(())
    }

    pub fn history(
        &self,
        limit: usize,
    ) -> Result<Vec<liquidcad_store::RevisionSummary>, EngineError> {
        match (&self.store, &self.project) {
            (Some(store), Some(project)) => Ok(store.history(&project.id, limit)?),
            _ => Ok(Vec::new()),
        }
    }

    pub fn restore(&mut self, revision: i64) -> Result<(), EngineError> {
        let store = self.store.as_ref().ok_or_else(|| {
            EngineError::Session("history is disabled (no data directory)".into())
        })?;
        let snapshot = store.snapshot(&self.project()?.id, revision)?;
        self.apply(format!("restore revision {revision}"), |p| {
            *p = snapshot;
            Ok(())
        })
    }

    pub fn export(&mut self, format: ExportFormat, path: &Path) -> Result<u64, EngineError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let document = self.document()?;
        // A part whose feature could not be built is shown unmodified; its
        // exports would silently omit the feature, so they are refused.
        if let Some(blocked) = document.diagnostics.iter().find(|d| {
            matches!(d.code.as_str(), "OCCT-01" | "FEAT-01") && d.severity == Severity::Error
        }) {
            return Err(EngineError::Session(format!(
                "export blocked until this is resolved: {}",
                blocked.message
            )));
        }
        let bytes = match format {
            ExportFormat::Dxf => liquidcad_dxf::write_dxf(document, path)?,
            ExportFormat::Csv => liquidcad_mfg::write_csv(document, path)?,
            ExportFormat::Pdf => liquidcad_mfg::write_pdf(document, path)?,
            ExportFormat::Step => {
                let name = document.project.name.clone();
                self.kernel.write_step(path, &name)?
            }
        };
        Ok(bytes)
    }

    pub fn save(&mut self, path: Option<&Path>) -> Result<PathBuf, EngineError> {
        let target = match path {
            Some(path) => path.to_path_buf(),
            None => self.project_path.clone().ok_or_else(|| {
                EngineError::Session("project has no file yet; choose Save As".into())
            })?,
        };
        let json = self.project()?.to_json()?;
        if let Some(parent) = target.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let tmp = target.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &target)?;
        self.project_path = Some(target.clone());
        self.dirty = false;
        Ok(target)
    }

    pub fn document(&self) -> Result<&Document, EngineError> {
        self.document
            .as_ref()
            .ok_or_else(|| EngineError::Session("no project is open".into()))
    }

    pub fn project(&self) -> Result<&Project, EngineError> {
        self.project
            .as_ref()
            .ok_or_else(|| EngineError::Session("no project is open".into()))
    }

    pub fn last_report(&self) -> &RebuildReport {
        &self.last_report
    }

    pub fn state(&self) -> Result<SessionState, EngineError> {
        let mut document = self.document()?.clone();
        document.scene.meshes = self.kernel.meshes();
        Ok(SessionState {
            document,
            selected: self.selected.clone(),
            kernel: liquidcad_occt::kernel_info().name,
            project_path: self.project_path.as_ref().map(|p| p.display().to_string()),
            dirty: self.dirty,
            can_undo: !self.undo.is_empty(),
            can_redo: !self.redo.is_empty(),
            undo_label: self.undo.last().map(|(_, n)| n.clone()),
            redo_label: self.redo.last().map(|(_, n)| n.clone()),
            rebuild: self.last_report.clone(),
            meshes_built: self.last_sync.built,
            meshes_reused: self.last_sync.reused,
        })
    }

    pub fn selection_info(&self) -> Result<Option<SelectionInfo>, EngineError> {
        let Some(id) = &self.selected else {
            return Ok(None);
        };
        let document = self.document()?;
        let project = self.project()?;
        let component = document.component(id)?.clone();
        let feature = project
            .feature(&component.feature_id)
            .ok_or_else(|| liquidcad_core::Error::UnknownFeature(component.feature_id.0.clone()))?;
        let material = project
            .materials
            .get(&component.material)
            .cloned()
            .ok_or_else(|| liquidcad_core::Error::UnknownMaterial(component.material.clone()))?;
        let mut depends_on: Vec<FeatureId> = Vec::new();
        let mut frontier = vec![feature.id.clone()];
        while let Some(next) = frontier.pop() {
            if let Some(f) = project.feature(&next) {
                for parent in &f.parents {
                    if !depends_on.contains(parent) {
                        depends_on.push(parent.clone());
                        frontier.insert(0, parent.clone());
                    }
                }
            }
        }
        let mut driven_by: Vec<String> = feature.inputs.clone();
        for upstream in &depends_on {
            if let Some(f) = project.feature(upstream) {
                for input in &f.inputs {
                    if !driven_by.contains(input) {
                        driven_by.push(input.clone());
                    }
                }
            }
        }
        Ok(Some(SelectionInfo {
            volume_in3: component.bounds.volume_in3(),
            movable: component.kind.movable(),
            feature_name: feature.name.clone(),
            component,
            material,
            depends_on,
            driven_by,
        }))
    }
}

fn snap(v: f64) -> f64 {
    (v / MOVE_SNAP_IN).round() * MOVE_SNAP_IN
}

fn build(
    evaluator: &mut Evaluator,
    kernel: &mut Kernel,
    project: &Project,
    revision: u64,
) -> Result<(Document, RebuildReport, SyncStats), EngineError> {
    let (mut document, report) = evaluator.rebuild(project, revision)?;
    let plan = liquidcad_mfg::cutlist::plan(&document);
    document
        .diagnostics
        .extend(liquidcad_mfg::cutlist::validate(&document, &plan));
    let colors: std::collections::HashMap<&str, [f32; 3]> = project
        .materials
        .iter()
        .map(|(k, m)| (k.as_str(), m.color))
        .collect();
    let sync = kernel.sync(&document.components, |c| {
        colors
            .get(c.material.as_str())
            .copied()
            .unwrap_or([0.6, 0.6, 0.6])
    })?;
    Ok((document, report, sync))
}

fn invalid(message: String) -> EngineError {
    EngineError::Core(liquidcad_core::Error::Validation(message))
}

/// The bundled sample, resolved relative to the workspace at build time.
pub fn default_project_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../projects/modular-dock/liquidcad.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use liquidcad_core::document::ComponentKind;
    use liquidcad_core::templates::modular_dock;

    fn engine() -> Engine {
        let mut engine = Engine::new(EngineConfig {
            data_dir: Some(std::env::temp_dir().join(format!(
                "liquidcad-engine-{}-{}",
                std::process::id(),
                rand_suffix()
            ))),
        })
        .unwrap();
        engine.load(modular_dock(), None).unwrap();
        engine
    }

    /// Unique per engine in this process. A clock-based name collided when
    /// parallel tests started in the same tick and shared one history DB.
    fn rand_suffix() -> usize {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        NEXT.fetch_add(1, Ordering::Relaxed)
    }

    fn floats(engine: &Engine) -> usize {
        engine
            .document()
            .unwrap()
            .components
            .iter()
            .filter(|c| c.kind == ComponentKind::FloatModule)
            .count()
    }

    fn bracket_draft(plane: SketchPlane) -> FeatureDraft {
        use liquidcad_core::feature::ExtrudeSpec;
        use liquidcad_core::geometry::{PathVertex, Profile};
        FeatureDraft {
            name: "Ladder bracket".into(),
            material: None,
            spec: FeatureSpec::Extrude(ExtrudeSpec {
                plane,
                profile: Profile {
                    outer: [[0., 0.], [6., 0.], [6., 2.], [2., 2.], [2., 6.], [0., 6.]]
                        .iter()
                        .map(|p| PathVertex::line(p[0], p[1]))
                        .collect(),
                    holes: vec![],
                },
                depth: 0.5,
                depth_parameter: None,
                direction: Default::default(),
                cut_into: None,
            }),
        }
    }

    #[test]
    fn sketched_parts_are_added_previewed_undone_and_exported() {
        let mut engine = engine();
        let plane = SketchPlane::Face {
            component: "deck.board.6".into(),
            face: "z+".into(),
        };
        let preview = engine
            .preview_feature(bracket_draft(plane.clone()))
            .unwrap();
        assert_eq!(preview.meshes.len(), 1);
        assert!(preview.mass.unwrap().mass_lb > 0.0);
        assert!(engine
            .document()
            .unwrap()
            .components
            .iter()
            .all(|c| !c.id.as_str().starts_with("custom.")));

        let id = engine.add_feature(bracket_draft(plane)).unwrap();
        assert_eq!(id.as_str(), "feat.extrude.1");
        let feature = engine.project().unwrap().feature(&id).unwrap().clone();
        assert_eq!(feature.parents[0].as_str(), "feat.deck");
        let doc = engine.document().unwrap();
        assert!(doc
            .components
            .iter()
            .any(|c| c.id.as_str() == "custom.extrude.1"));
        assert!(doc.bom.iter().any(|l| l.kind == "custom"));
        // Changing the deck regenerates the bracket that sits on it.
        engine.set_parameter("deck_thickness_in", 1.5).unwrap();
        assert!(engine
            .last_report()
            .regenerated
            .iter()
            .any(|f| f.as_str() == "feat.extrude.1"));

        let dir = std::env::temp_dir().join(format!("liquidcad-custom-{}", std::process::id()));
        let dxf = dir.join("custom.dxf");
        engine.export(ExportFormat::Dxf, &dxf).unwrap();
        let text = std::fs::read_to_string(&dxf).unwrap();
        assert!(text.contains("LADDER BRACKET x1  T 0.5 IN"));
        let summary = liquidcad_dxf::check::parse(&text).unwrap();
        assert!(summary.entity_layers.iter().any(|l| l == "CUSTOM"));

        engine.undo().unwrap();
        engine.undo().unwrap();
        assert!(engine.project().unwrap().feature(&id).is_none());
        engine.redo().unwrap();
        assert!(engine.project().unwrap().feature(&id).is_some());
        engine.delete_feature(id.as_str()).unwrap();
        assert!(engine.project().unwrap().feature(&id).is_none());
    }

    #[test]
    fn broken_sketches_never_reach_history() {
        let mut engine = engine();
        let mut draft = bracket_draft(SketchPlane::Face {
            component: "deck.board.6".into(),
            face: "nope".into(),
        });
        let err = engine.add_feature(draft.clone()).unwrap_err().to_string();
        assert!(err.contains("face nope does not exist"), "{err}");
        if let FeatureSpec::Extrude(spec) = &mut draft.spec {
            spec.plane = SketchPlane::Xy { offset: 0.0 };
            spec.profile.outer.swap(1, 3);
        }
        assert!(engine.add_feature(draft).is_err());
        assert!(!engine.state().unwrap().can_undo);
    }

    #[test]
    fn modifier_needs_opencascade_or_builds() {
        use liquidcad_core::feature::ModifySpec;
        use liquidcad_core::geometry::ShapeOp;
        let mut engine = engine();
        let draft = FeatureDraft {
            name: "Bolt hole".into(),
            material: None,
            spec: FeatureSpec::Modify(ModifySpec {
                target: "frame.stringer.port".into(),
                // Tube section local (x, y) = (across, up), z along the tube
                // from its bow end: side.3 is the port wall (x = 0).
                op: ShapeOp::Hole {
                    face: "side.3".into(),
                    at: [0.0, 2.0, 168.0],
                    diameter: 0.5,
                    depth: 0.0,
                },
            }),
        };
        let before = engine
            .document()
            .unwrap()
            .component(&ComponentId::new("frame.stringer.port"))
            .unwrap()
            .mass_lb();
        let result = engine.add_feature(draft);
        if liquidcad_occt::kernel_info().native_occt {
            result.unwrap();
            let c = engine
                .document()
                .unwrap()
                .component(&ComponentId::new("frame.stringer.port"))
                .unwrap()
                .clone();
            // A 1/2 in hole through both 1/8 in walls of the tube.
            let density = 169.0 / 1728.0;
            let removed = std::f64::consts::PI * 0.0625 * 0.25 * density;
            assert!(
                (before - c.mass_lb() - removed).abs() < 1e-6,
                "{}",
                before - c.mass_lb()
            );
            let mesh = engine.kernel.mesh(&c.id).unwrap();
            assert!(mesh.faces.iter().any(|f| f.id == "hole1.wall"));
        } else {
            let err = result.unwrap_err().to_string();
            assert!(err.contains("needs OpenCascade"), "{err}");
        }
    }

    #[test]
    fn gangway_slope_moves_only_the_gangway() {
        let mut engine = engine();
        engine.set_parameter("gangway_angle_deg", 12.0).unwrap();
        let sync = engine.last_sync;
        let gangway = engine
            .document()
            .unwrap()
            .components
            .iter()
            .filter(|c| c.id.as_str().starts_with("gangway."))
            .count();
        assert_eq!(sync.built, gangway, "{sync:?}");
        let doc = engine.document().unwrap();
        assert!(doc.diagnostics.iter().any(|d| d.code == "GANG-01"));
    }

    #[test]
    fn simulation_runs_on_the_current_design() {
        let engine = engine();
        let result = engine
            .simulate(&liquidcad_sim::SimParams {
                wave_height_in: 12.0,
                period_s: 2.5,
                heading_deg: 90.0,
                duration_s: 10.0,
                walker: Some(liquidcad_sim::Walker {
                    weight_lb: 200.0,
                    from: [10.0, 20.0],
                    to: [86.0, 170.0],
                    speed_in_s: 40.0,
                }),
            })
            .unwrap();
        assert_eq!(result.frames.len(), 301);
        let e = &engine.document().unwrap().engineering;
        assert!((result.summary.still_draft_in - e.draft_dead_in).abs() < 0.05);
        assert!(result.summary.max_roll_deg > 0.5);
        assert!(result.frames.iter().all(|f| f.walker.is_some()));
    }

    #[test]
    fn edit_undo_redo_round_trip() {
        let mut engine = engine();
        engine.select(Some("frame.stringer.stbd")).unwrap();
        engine.set_parameter("width_in", 144.0).unwrap();
        assert_eq!(floats(&engine), 48);
        assert_eq!(
            engine.state().unwrap().selected.unwrap().0,
            "frame.stringer.stbd"
        );
        assert_eq!(
            engine.state().unwrap().undo_label.as_deref(),
            Some("Dock width = 144")
        );
        engine.undo().unwrap();
        assert_eq!(floats(&engine), 32);
        engine.redo().unwrap();
        assert_eq!(floats(&engine), 48);
        let history = engine.history(10).unwrap();
        assert_eq!(history[0].note, "redo Dock width = 144");
        assert_eq!(history.len(), 4);
    }

    #[test]
    fn rejected_edit_changes_nothing() {
        let mut engine = engine();
        let before = engine.state().unwrap().document.revision;
        let err = engine.set_parameter("width_in", 100.0).unwrap_err();
        assert_eq!(err.code(), "validation");
        assert_eq!(engine.state().unwrap().document.revision, before);
        assert!(!engine.state().unwrap().can_undo);
    }

    #[test]
    fn selection_reports_dependencies() {
        let mut engine = engine();
        engine.select(Some("deck.board.0")).unwrap();
        let info = engine.selection_info().unwrap().unwrap();
        assert_eq!(info.feature_name, "Deck boards");
        assert!(info.depends_on.iter().any(|f| f.0 == "feat.frame"));
        assert!(info.driven_by.contains(&"width_in".to_string()));
        assert!(engine.select(Some("nope")).is_err());
    }

    #[test]
    fn moves_snap_and_stay_on_deck() {
        let mut engine = engine();
        engine
            .move_part("cleat.port.mid", [0.0, 12.3, 4.0])
            .unwrap();
        let doc = engine.document().unwrap();
        let cleat = doc.component(&ComponentId::new("cleat.port.mid")).unwrap();
        assert_eq!(cleat.placement, [0.0, 12.5, 0.0]);
        assert!(engine
            .move_part("cleat.port.mid", [-50.0, 0.0, 0.0])
            .is_err());
        assert!(engine.move_part("float.r0.c0", [1.0, 0.0, 0.0]).is_err());
        // Ids are stable, so the placement survives a rebuild that resizes the dock.
        engine.set_parameter("width_in", 120.0).unwrap();
        let cleat = engine
            .document()
            .unwrap()
            .component(&ComponentId::new("cleat.port.mid"))
            .unwrap()
            .clone();
        assert_eq!(cleat.placement, [0.0, 12.5, 0.0]);
    }

    #[test]
    fn restore_reverts_through_undo() {
        let mut engine = engine();
        let first = engine.history(1).unwrap()[0].id;
        engine.set_parameter("length_in", 240.0).unwrap();
        engine.restore(first).unwrap();
        assert_eq!(
            engine.project().unwrap().parameters["length_in"].value,
            192.0
        );
        engine.undo().unwrap();
        assert_eq!(
            engine.project().unwrap().parameters["length_in"].value,
            240.0
        );
    }

    #[test]
    fn exports_every_format_and_follows_edits() {
        let mut engine = engine();
        let dir = std::env::temp_dir().join(format!("liquidcad-export-{}", rand_suffix()));
        engine.set_parameter("width_in", 144.0).unwrap();
        for format in ExportFormat::ALL {
            let path = dir.join(format!("dock.{}", format.extension()));
            let bytes = engine.export(format, &path).unwrap();
            assert!(bytes > 500, "{format:?} only {bytes} bytes");
        }
        let csv = std::fs::read_to_string(dir.join("dock.csv")).unwrap();
        assert!(csv.contains(",float_module,") && csv.contains(",48,"));
        let step = std::fs::read_to_string(dir.join("dock.step")).unwrap();
        let parsed = liquidcad_occt::step::check::parse(&step).unwrap();
        let solids = parsed
            .values()
            .filter(|i| i.keywords.iter().any(|k| k == "MANIFOLD_SOLID_BREP"))
            .count();
        assert_eq!(solids, engine.document().unwrap().components.len());
        liquidcad_dxf::check::parse(&std::fs::read_to_string(dir.join("dock.dxf")).unwrap())
            .unwrap();
        assert!(std::fs::read(dir.join("dock.pdf"))
            .unwrap()
            .starts_with(b"%PDF-1.4"));
    }

    #[test]
    fn save_writes_a_loadable_project() {
        let mut engine = engine();
        engine.set_parameter("width_in", 120.0).unwrap();
        let path = std::env::temp_dir().join(format!("liquidcad-save-{}.json", rand_suffix()));
        engine.save(Some(&path)).unwrap();
        assert!(!engine.state().unwrap().dirty);
        let loaded = Project::load_path(&path).unwrap();
        assert_eq!(loaded.parameters["width_in"].value, 120.0);
    }

    #[test]
    fn protocol_dispatch_reports_codes() {
        let mut engine = Engine::new(EngineConfig::default()).unwrap();
        let response = engine.handle(9, Request::Document);
        assert_eq!(response.error.unwrap().code, "session");
        let response = engine.handle(10, Request::Ping);
        assert!(matches!(response.result, Some(Value::Pong(_))));
    }
}
