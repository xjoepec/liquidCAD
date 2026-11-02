// Imperative Three.js scene for the model workspace. React owns the
// lifecycle; this class owns everything per-frame.
import {
  AxesHelper,
  BufferAttribute,
  BufferGeometry,
  Color,
  ConeGeometry,
  DirectionalLight,
  DoubleSide,
  Group,
  HemisphereLight,
  Line,
  LineBasicMaterial,
  LineSegments,
  Mesh,
  MeshBasicMaterial,
  MeshStandardMaterial,
  Object3D,
  OrthographicCamera,
  PerspectiveCamera,
  PlaneGeometry,
  Quaternion,
  Raycaster,
  Scene,
  SphereGeometry,
  SRGBColorSpace,
  Vector2,
  Vector3,
  WebGLRenderer,
  GridHelper,
  type Camera,
} from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { TransformControls } from "three/examples/jsm/controls/TransformControls.js";
import type { PickMode, TopoPick, Tool, ViewName } from "../store";
import type { Document, PreviewResult, SimFrame, SimResult } from "../types";
import { PALETTE, oklch } from "../../shared/color";
import { isClick, measure, nearestScreenPoint, snapMove, uniqueVertices, type Measurement, type Vec3 } from "./snap";

Object3D.DEFAULT_UP.set(0, 0, 1);

const ACCENT = new Color(PALETTE.accent);
const HOVER = new Color(PALETTE.hover);
const EDGE = new Color(PALETTE.edge);

type MeshSrc = Document["scene"]["meshes"][number];

type Part = {
  id: string;
  key: string;
  mesh: Mesh<BufferGeometry, MeshStandardMaterial>;
  edges: LineSegments<BufferGeometry, LineBasicMaterial>;
  center: Vector3;
  corners: Vec3[];
  src: MeshSrc;
};

export type TopoHit = Omit<TopoPick, "component">;

const PICK = new Color(PALETTE.pick);
const PICK_HOVER = new Color(PALETTE.pickHover);
const EDGE_PICK_PX = 10;

export type SceneCallbacks = {
  onPick: (id: string | null, topo: TopoHit | null, additive: boolean) => void;
  onHover: (id: string | null, topo: TopoHit | null) => void;
  onMove: (id: string, delta: Vec3) => void;
  onMeasure: (m: (Measurement & { a: Vec3; b: Vec3 }) | null) => void;
};

const VIEW_DIRS: Record<ViewName, Vector3> = {
  iso: new Vector3(1, -1.25, 0.95),
  top: new Vector3(0, -0.0001, 1),
  front: new Vector3(0, -1, 0.0001),
  right: new Vector3(1, 0, 0.0001),
};

export class CadScene {
  readonly renderer: WebGLRenderer;
  private scene = new Scene();
  private persp = new PerspectiveCamera(40, 1, 0.5, 50_000);
  private ortho = new OrthographicCamera(-1, 1, 1, -1, -50_000, 50_000);
  private camera: PerspectiveCamera | OrthographicCamera = this.persp;
  private controls: OrbitControls;
  private transform: TransformControls;
  private transformHelper: Object3D;
  private root = new Group();
  private parts = new Map<string, Part>();
  private water: Mesh<PlaneGeometry, MeshStandardMaterial>;
  private grid: GridHelper;
  private raycaster = new Raycaster();
  private key = new DirectionalLight(oklch(1, 0, 0), 2.4);
  private axesScene = new Scene();
  private axesCamera = new OrthographicCamera(-1.4, 1.4, 1.4, -1.4, 0.1, 10);
  private marker: Mesh<SphereGeometry, MeshBasicMaterial>;
  private measureLine: Line<BufferGeometry, LineBasicMaterial>;
  private measurePoints: Vec3[] = [];
  private selected: string | null = null;
  private hovered: string | null = null;
  private tool: Tool = "select";
  private movable = new Set<string>();
  private down: [number, number] | null = null;
  private dragging = false;
  private resizeObserver: ResizeObserver;
  private span = 200;
  private hoverFrame = 0;
  private pickMode: PickMode = "part";
  private picks: TopoPick[] = [];
  private hoverTopo: TopoPick | null = null;
  private topoLayer = new Group();
  private previewLayer = new Group();
  private markerLayer = new Group();
  private mateLayer = new Group();
  private waveSurface: Mesh<PlaneGeometry, MeshStandardMaterial>;
  private walker: Mesh<SphereGeometry, MeshBasicMaterial>;
  private showMass = false;
  private doc: Document | null = null;
  revision = -1;

  constructor(
    private readonly host: HTMLElement,
    private readonly cb: SceneCallbacks,
  ) {
    this.renderer = new WebGLRenderer({ antialias: true, preserveDrawingBuffer: true });
    this.renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    this.renderer.outputColorSpace = SRGBColorSpace;
    this.renderer.autoClear = false;
    host.appendChild(this.renderer.domElement);
    this.scene.background = new Color(PALETTE.canvas);

    this.scene.add(new HemisphereLight(PALETTE.sky, PALETTE.ground, 1.5));
    this.key.position.set(1, -1.6, 2.4);
    this.scene.add(this.key);
    const fill = new DirectionalLight(PALETTE.fill, 0.6);
    fill.position.set(-1.5, 1, 0.6);
    this.scene.add(fill);
    this.scene.add(this.root);
    this.root.add(this.topoLayer, this.previewLayer, this.markerLayer, this.mateLayer);

    this.grid = new GridHelper(1, 1, PALETTE.gridMajor, PALETTE.gridMinor);
    this.grid.rotation.x = Math.PI / 2;
    this.scene.add(this.grid);

    this.water = new Mesh(
      new PlaneGeometry(1, 1),
      new MeshStandardMaterial({ color: PALETTE.water, transparent: true, opacity: 0.16, roughness: 0.3, side: DoubleSide, depthWrite: false }),
    );
    this.water.visible = false;
    this.water.renderOrder = 2;
    this.scene.add(this.water);

    this.waveSurface = new Mesh(
      new PlaneGeometry(1, 1, 96, 96),
      new MeshStandardMaterial({ color: PALETTE.water, transparent: true, opacity: 0.45, roughness: 0.25, side: DoubleSide, depthWrite: false }),
    );
    this.waveSurface.visible = false;
    this.waveSurface.renderOrder = 2;
    this.scene.add(this.waveSurface);
    this.walker = new Mesh(new SphereGeometry(6, 16, 12), new MeshBasicMaterial({ color: PALETTE.walker }));
    this.walker.visible = false;
    this.root.add(this.walker);

    this.marker = new Mesh(new SphereGeometry(1, 16, 12), new MeshBasicMaterial({ color: PALETTE.pick, depthTest: false }));
    this.marker.renderOrder = 10;
    this.marker.visible = false;
    this.scene.add(this.marker);
    this.measureLine = new Line(new BufferGeometry(), new LineBasicMaterial({ color: PALETTE.pick, depthTest: false }));
    this.measureLine.renderOrder = 10;
    this.measureLine.visible = false;
    this.scene.add(this.measureLine);

    this.controls = new OrbitControls(this.camera, this.renderer.domElement);
    this.controls.enableDamping = true;
    this.controls.dampingFactor = 0.12;
    this.controls.screenSpacePanning = true;
    this.controls.zoomToCursor = true;

    this.transform = new TransformControls(this.camera, this.renderer.domElement);
    this.transform.setMode("translate");
    this.transform.setSpace("world");
    this.transform.showZ = false;
    this.transform.setTranslationSnap(0.5);
    this.transform.size = 0.8;
    this.transformHelper = (this.transform as unknown as { getHelper?: () => Object3D }).getHelper?.() ?? (this.transform as unknown as Object3D);
    this.scene.add(this.transformHelper);
    this.transform.addEventListener("dragging-changed", (event) => {
      const active = Boolean((event as unknown as { value: boolean }).value);
      this.controls.enabled = !active;
      this.dragging = active;
      if (!active) this.finishMove();
    });

    const axes = new AxesHelper(1);
    this.axesScene.add(axes);
    this.axesCamera.up.set(0, 0, 1);

    const el = this.renderer.domElement;
    el.addEventListener("pointerdown", this.onPointerDown);
    el.addEventListener("pointerup", this.onPointerUp);
    el.addEventListener("pointermove", this.onPointerMove);
    el.addEventListener("pointerleave", this.onPointerLeave);
    this.resizeObserver = new ResizeObserver(() => this.resize());
    this.resizeObserver.observe(host);
    this.resize();
    this.renderer.setAnimationLoop(() => this.frame());
  }

  dispose(): void {
    this.renderer.setAnimationLoop(null);
    this.resizeObserver.disconnect();
    const el = this.renderer.domElement;
    el.removeEventListener("pointerdown", this.onPointerDown);
    el.removeEventListener("pointerup", this.onPointerUp);
    el.removeEventListener("pointermove", this.onPointerMove);
    el.removeEventListener("pointerleave", this.onPointerLeave);
    this.transform.detach();
    this.transform.dispose();
    this.controls.dispose();
    for (const part of this.parts.values()) this.disposePart(part);
    this.renderer.dispose();
    el.remove();
  }

  get meshCount(): number {
    return this.parts.size;
  }

  /** Reconciles parts with the document. Unchanged solids keep their GPU buffers. */
  setDocument(doc: Document | null): void {
    if (!doc) {
      for (const part of this.parts.values()) this.disposePart(part);
      this.parts.clear();
      this.revision = -1;
      return;
    }
    this.doc = doc;
    const solids = new Map(doc.components.map((c) => [c.id, c]));
    const seen = new Set<string>();
    this.movable = new Set(doc.components.filter((c) => c.kind === "cleat").map((c) => c.id));
    for (const src of doc.scene.meshes) {
      const component = solids.get(src.component_id);
      const key = `${JSON.stringify(component?.shape)}|${JSON.stringify(component?.pose)}|${src.color.join(",")}`;
      seen.add(src.component_id);
      const existing = this.parts.get(src.component_id);
      if (existing && existing.key === key) continue;
      if (existing) this.disposePart(existing);
      this.parts.set(src.component_id, this.buildPart(src, key));
    }
    for (const [id, part] of this.parts) {
      if (!seen.has(id)) {
        this.disposePart(part);
        this.parts.delete(id);
      }
    }
    const [min, max] = [doc.scene.bounds_min, doc.scene.bounds_max];
    const size = new Vector3(max[0] - min[0], max[1] - min[1], max[2] - min[2]);
    this.span = Math.max(size.x, size.y, size.z, 24);
    const center = new Vector3((min[0] + max[0]) / 2, (min[1] + max[1]) / 2, 0);
    const extent = Math.max(size.x, size.y) * 1.8;
    this.water.scale.set(extent, extent, 1);
    this.water.position.set(center.x, center.y, doc.engineering.draft_dead_in);
    this.water.visible = Number.isFinite(doc.engineering.draft_dead_in) && doc.engineering.float_count > 0;
    const gridSize = Math.ceil((extent * 1.4) / 24) * 24;
    this.grid.scale.set(gridSize, 1, gridSize);
    this.grid.position.set(center.x, center.y, -0.05);
    this.key.position.set(center.x + this.span, center.y - this.span * 1.6, this.span * 2.4);
    this.revision = doc.revision;
    this.applyStyles();
    this.buildMates(doc);
    this.buildMarkers();
    this.buildTopo();
    this.waveSurface.scale.set(extent, extent, 1);
    this.waveSurface.position.set(center.x, center.y, 0);
    if (this.selected && !this.parts.has(this.selected)) this.setSelection(null);
    else this.syncTransform();
  }

  setPickMode(mode: PickMode): void {
    this.pickMode = mode;
    if (mode === "part") this.setHoverTopo(null);
  }

  setPicks(picks: TopoPick[]): void {
    this.picks = picks;
    this.buildTopo();
  }

  setHoverTopo(pick: TopoPick | null): void {
    const same = (a: TopoPick | null, b: TopoPick | null) =>
      a?.component === b?.component && a?.kind === b?.kind && a?.id === b?.id;
    if (same(pick, this.hoverTopo)) return;
    this.hoverTopo = pick;
    this.buildTopo();
  }

  setShowMass(show: boolean): void {
    this.showMass = show;
    this.buildMarkers();
  }

  /** Draft feature parts, drawn translucent over the model. */
  setPreview(preview: PreviewResult | null): void {
    for (const child of [...this.previewLayer.children]) {
      this.previewLayer.remove(child);
      disposeObject(child);
    }
    for (const src of preview?.meshes ?? []) {
      const geometry = new BufferGeometry();
      geometry.setAttribute("position", new BufferAttribute(new Float32Array(src.positions), 3));
      geometry.setAttribute("normal", new BufferAttribute(new Float32Array(src.normals), 3));
      geometry.setIndex(src.indices);
      const mesh = new Mesh(
        geometry,
        new MeshStandardMaterial({ color: PALETTE.accent, transparent: true, opacity: 0.55, emissive: PALETTE.accentGlow, depthWrite: false }),
      );
      mesh.renderOrder = 4;
      const wire = new BufferGeometry();
      wire.setAttribute("position", new BufferAttribute(new Float32Array(src.edges), 3));
      this.previewLayer.add(mesh, new LineSegments(wire, new LineBasicMaterial({ color: PALETTE.accent })));
    }
  }

  /** Plays a simulation frame: the whole model moves as one rigid body. */
  setSimulation(sim: SimResult | null, t: number): void {
    if (!sim || sim.frames.length === 0) {
      this.root.position.set(0, 0, 0);
      this.root.quaternion.identity();
      this.waveSurface.visible = false;
      this.walker.visible = false;
      this.water.visible = Boolean(this.doc && this.doc.engineering.float_count > 0);
      return;
    }
    const f = frameAt(sim, t);
    const pivot = new Vector3(...sim.pivot);
    // Heel (starboard down) turns about +y; trim (bow down) turns about -x.
    const q = new Quaternion()
      .setFromAxisAngle(new Vector3(0, 1, 0), (f.roll_deg * Math.PI) / 180)
      .multiply(new Quaternion().setFromAxisAngle(new Vector3(1, 0, 0), (-f.pitch_deg * Math.PI) / 180));
    this.root.quaternion.copy(q);
    // The model is drawn at rest (float bottoms at z = 0, still water at
    // the draft); frames are motions about that rest, so heave adds directly.
    const p = pivot.clone().applyQuaternion(q);
    this.root.position.set(pivot.x - p.x, pivot.y - p.y, pivot.z - p.z + f.heave_in);
    this.water.visible = false;
    this.waveSurface.visible = true;
    const pos = this.waveSurface.geometry.getAttribute("position") as BufferAttribute;
    const sx = this.waveSurface.scale.x;
    const sy = this.waveSurface.scale.y;
    const cx = this.waveSurface.position.x;
    const cy = this.waveSurface.position.y;
    const params = sim.params;
    for (let i = 0; i < pos.count; i++) {
      const x = cx + pos.getX(i) * sx;
      const y = cy + pos.getY(i) * sy;
      pos.setZ(i, sim.water_level_in + (params ? waveEta(params, x, y, t) : 0));
    }
    pos.needsUpdate = true;
    this.waveSurface.geometry.computeVertexNormals();
    this.waveSurface.position.z = 0;
    if (f.walker && this.doc) {
      this.walker.visible = true;
      this.walker.position.set(f.walker[0], f.walker[1], this.doc.engineering.deck_top_in + 6);
    } else {
      this.walker.visible = false;
    }
  }

  private buildMates(doc: Document): void {
    for (const child of [...this.mateLayer.children]) {
      this.mateLayer.remove(child);
      disposeObject(child);
    }
    for (const m of doc.mates) {
      const dir = new Vector3(...m.direction);
      const origin = new Vector3(...m.origin);
      const half = Math.max(m.length * 0.6, 12);
      const a = origin.clone().addScaledVector(dir, -half);
      const b = origin.clone().addScaledVector(dir, half);
      const geometry = new BufferGeometry().setFromPoints([a, b]);
      const line = new Line(geometry, new LineBasicMaterial({ color: PALETTE.mate, depthTest: false }));
      line.renderOrder = 8;
      const cone = new Mesh(new ConeGeometry(1.6, 5, 16), new MeshBasicMaterial({ color: PALETTE.mate, depthTest: false }));
      cone.position.copy(b);
      cone.quaternion.setFromUnitVectors(new Vector3(0, 1, 0), dir);
      cone.renderOrder = 8;
      line.userData.mate = m.id;
      this.mateLayer.add(line, cone);
    }
  }

  private buildMarkers(): void {
    for (const child of [...this.markerLayer.children]) {
      this.markerLayer.remove(child);
      disposeObject(child);
    }
    if (!this.showMass || !this.doc) return;
    const e = this.doc.engineering;
    const r = Math.max(2, this.span * 0.012);
    const add = (p: Vec3, color: string) => {
      const s = new Mesh(new SphereGeometry(r, 20, 14), new MeshBasicMaterial({ color, depthTest: false }));
      s.position.set(...p);
      s.renderOrder = 9;
      const stem = new Line(
        new BufferGeometry().setFromPoints([new Vector3(p[0], p[1], p[2]), new Vector3(p[0], p[1], e.draft_dead_in)]),
        new LineBasicMaterial({ color, depthTest: false }),
      );
      stem.renderOrder = 9;
      this.markerLayer.add(s, stem);
    };
    add(e.center_of_gravity, PALETTE.gravity);
    add(e.center_of_buoyancy, PALETTE.buoyancy);
  }

  /** Overlays for picked and hovered faces and edges. */
  private buildTopo(): void {
    for (const child of [...this.topoLayer.children]) {
      this.topoLayer.remove(child);
      disposeObject(child);
    }
    const draw = (pick: TopoPick, color: Color, opacity: number) => {
      const part = this.parts.get(pick.component);
      if (!part) return;
      if (pick.kind === "face") {
        const face = part.src.faces.find((f) => f.id === pick.id);
        if (!face) return;
        const idx = part.src.indices.slice(face.tri_start * 3, (face.tri_start + face.tri_count) * 3);
        const pts = new Float32Array(idx.length * 3);
        idx.forEach((v, i) => {
          pts[i * 3] = part.src.positions[v * 3]!;
          pts[i * 3 + 1] = part.src.positions[v * 3 + 1]!;
          pts[i * 3 + 2] = part.src.positions[v * 3 + 2]!;
        });
        const geometry = new BufferGeometry();
        geometry.setAttribute("position", new BufferAttribute(pts, 3));
        const mesh = new Mesh(
          geometry,
          new MeshBasicMaterial({
            color,
            transparent: true,
            opacity,
            side: DoubleSide,
            depthWrite: false,
            polygonOffset: true,
            polygonOffsetFactor: -2,
            polygonOffsetUnits: -2,
          }),
        );
        mesh.renderOrder = 6;
        this.topoLayer.add(mesh);
      } else {
        const edge = part.src.edge_info.find((e) => e.id === pick.id);
        if (!edge) return;
        const pts: number[] = [];
        for (let i = 0; i + 5 < edge.polyline.length; i += 3) pts.push(...edge.polyline.slice(i, i + 6));
        const geometry = new BufferGeometry();
        geometry.setAttribute("position", new BufferAttribute(new Float32Array(pts), 3));
        const line = new LineSegments(geometry, new LineBasicMaterial({ color, depthTest: false }));
        line.renderOrder = 7;
        this.topoLayer.add(line);
      }
    };
    for (const pick of this.picks) draw(pick, PICK, 0.5);
    if (this.hoverTopo) draw(this.hoverTopo, PICK_HOVER, 0.3);
  }

  setSelection(id: string | null): void {
    this.selected = id;
    this.applyStyles();
    this.syncTransform();
  }

  setHover(id: string | null): void {
    if (this.hovered === id) return;
    this.hovered = id;
    this.applyStyles();
  }

  setTool(tool: Tool): void {
    this.tool = tool;
    if (tool !== "measure") this.clearMeasure();
    this.syncTransform();
  }

  setWaterVisible(visible: boolean): void {
    this.water.material.opacity = visible ? 0.22 : 0;
  }

  setOrtho(enabled: boolean): void {
    const target = this.controls.target.clone();
    const from = this.camera;
    const to = enabled ? this.ortho : this.persp;
    if (from === to) return;
    to.position.copy(from.position);
    to.quaternion.copy(from.quaternion);
    if (enabled) {
      const distance = from.position.distanceTo(target);
      const h = 2 * distance * Math.tan((this.persp.fov * Math.PI) / 360);
      this.ortho.userData.halfHeight = h / 2;
      this.ortho.zoom = 1;
    }
    this.camera = to;
    this.controls.object = to;
    this.transform.camera = to;
    this.resize();
    this.controls.target.copy(target);
    this.controls.update();
  }

  setView(name: ViewName): void {
    this.placeCamera(VIEW_DIRS[name].clone().normalize());
  }

  fit(): void {
    const dir = this.camera.position.clone().sub(this.controls.target).normalize();
    this.placeCamera(dir.lengthSq() > 0 ? dir : VIEW_DIRS.iso.clone().normalize());
  }

  /** Client-space centre of a part, for automation and tests. */
  screenPoint(id: string): { x: number; y: number } | null {
    const part = this.parts.get(id);
    if (!part) return null;
    const p = part.mesh.position.clone().project(this.camera);
    if (p.z < -1 || p.z > 1) return null;
    const rect = this.renderer.domElement.getBoundingClientRect();
    return { x: rect.left + ((p.x + 1) / 2) * rect.width, y: rect.top + ((1 - p.y) / 2) * rect.height };
  }

  private placeCamera(dir: Vector3): void {
    const center = this.boundsCenter();
    const radius = this.span * 0.62;
    const distance = radius / Math.sin((this.persp.fov * Math.PI) / 360);
    this.controls.target.copy(center);
    this.camera.position.copy(center).addScaledVector(dir, distance);
    if (this.camera === this.ortho) {
      this.ortho.userData.halfHeight = radius * 1.05;
      this.ortho.zoom = 1;
      this.resize();
    }
    this.camera.lookAt(center);
    this.controls.update();
  }

  private boundsCenter(): Vector3 {
    if (this.parts.size === 0) return new Vector3();
    const min = new Vector3(Infinity, Infinity, Infinity);
    const max = new Vector3(-Infinity, -Infinity, -Infinity);
    for (const part of this.parts.values()) {
      part.mesh.geometry.computeBoundingBox();
      const box = part.mesh.geometry.boundingBox!;
      min.min(box.min.clone().add(part.mesh.position));
      max.max(box.max.clone().add(part.mesh.position));
    }
    return min.add(max).multiplyScalar(0.5);
  }

  private buildPart(src: Document["scene"]["meshes"][number], key: string): Part {
    const positions = new Float32Array(src.positions);
    const corners = uniqueVertices(positions);
    const center = new Vector3();
    for (const c of corners) center.add(new Vector3(...c));
    center.multiplyScalar(1 / Math.max(1, corners.length));
    for (let i = 0; i < positions.length; i += 3) {
      positions[i]! -= center.x;
      positions[i + 1]! -= center.y;
      positions[i + 2]! -= center.z;
    }
    const geometry = new BufferGeometry();
    geometry.setAttribute("position", new BufferAttribute(positions, 3));
    geometry.setAttribute("normal", new BufferAttribute(new Float32Array(src.normals), 3));
    geometry.setIndex(src.indices);
    geometry.computeBoundingSphere();
    const material = new MeshStandardMaterial({
      color: new Color().setRGB(src.color[0], src.color[1], src.color[2], SRGBColorSpace),
      roughness: 0.72,
      metalness: 0.08,
      polygonOffset: true,
      polygonOffsetFactor: 1,
      polygonOffsetUnits: 1,
    });
    const mesh = new Mesh(geometry, material);
    mesh.position.copy(center);
    mesh.userData.id = src.component_id;
    const edgePositions = new Float32Array(src.edges);
    for (let i = 0; i < edgePositions.length; i += 3) {
      edgePositions[i]! -= center.x;
      edgePositions[i + 1]! -= center.y;
      edgePositions[i + 2]! -= center.z;
    }
    const edgeGeometry = new BufferGeometry();
    edgeGeometry.setAttribute("position", new BufferAttribute(edgePositions, 3));
    const edges = new LineSegments(edgeGeometry, new LineBasicMaterial({ color: EDGE, transparent: true, opacity: 0.55 }));
    mesh.add(edges);
    this.root.add(mesh);
    return { id: src.component_id, key, mesh, edges, center: center.clone(), corners, src };
  }

  /** Face or edge under the cursor for the current pick mode. */
  private topoAt(event: PointerEvent): { id: string; topo: TopoHit } | null {
    const hit = this.hit(event);
    if (this.pickMode === "face") {
      if (!hit || hit.faceIndex == null) return null;
      const part = this.parts.get(String(hit.object.userData.id));
      const tri = hit.faceIndex;
      const face = part?.src.faces.find((f) => tri >= f.tri_start && tri < f.tri_start + f.tri_count);
      if (!part || !face) return null;
      return { id: part.id, topo: { kind: "face", id: face.id, point: [hit.point.x, hit.point.y, hit.point.z] } };
    }
    if (this.pickMode === "edge") {
      const rect = this.renderer.domElement.getBoundingClientRect();
      const mouse = [event.clientX - rect.left, event.clientY - rect.top] as const;
      const hitPart = hit ? this.parts.get(String(hit.object.userData.id)) : undefined;
      const candidates = hitPart ? [hitPart] : [...this.parts.values()];
      let best: { d: number; part: Part; id: string; point: Vec3 } | null = null;
      const screen = (x: number, y: number, z: number) => {
        const p = new Vector3(x, y, z).applyMatrix4(this.root.matrixWorld).project(this.camera);
        return [((p.x + 1) / 2) * rect.width, ((1 - p.y) / 2) * rect.height, p.z] as const;
      };
      for (const part of candidates) {
        for (const edge of part.src.edge_info) {
          const pl = edge.polyline;
          for (let i = 0; i + 5 < pl.length; i += 3) {
            const a = screen(pl[i]!, pl[i + 1]!, pl[i + 2]!);
            const b = screen(pl[i + 3]!, pl[i + 4]!, pl[i + 5]!);
            if (a[2] < -1 || a[2] > 1 || b[2] < -1 || b[2] > 1) continue;
            const dx = b[0] - a[0];
            const dy = b[1] - a[1];
            const len2 = dx * dx + dy * dy;
            const t = len2 > 0 ? Math.max(0, Math.min(1, ((mouse[0] - a[0]) * dx + (mouse[1] - a[1]) * dy) / len2)) : 0;
            const d = Math.hypot(a[0] + t * dx - mouse[0], a[1] + t * dy - mouse[1]);
            if (d <= EDGE_PICK_PX && (!best || d < best.d)) {
              const w: Vec3 = [
                pl[i]! + t * (pl[i + 3]! - pl[i]!),
                pl[i + 1]! + t * (pl[i + 4]! - pl[i + 1]!),
                pl[i + 2]! + t * (pl[i + 5]! - pl[i + 2]!),
              ];
              best = { d, part, id: edge.id, point: w };
            }
          }
        }
      }
      return best ? { id: best.part.id, topo: { kind: "edge", id: best.id, point: best.point } } : null;
    }
    return null;
  }

  /** Client point over a named face's centroid or edge midpoint, for tests. */
  topoScreenPoint(component: string, kind: "face" | "edge", id: string): { x: number; y: number } | null {
    const part = this.parts.get(component);
    if (!part) return null;
    let p: Vector3 | null = null;
    if (kind === "face") {
      const f = part.src.faces.find((x) => x.id === id);
      if (f) p = new Vector3(...f.centroid);
    } else {
      const e = part.src.edge_info.find((x) => x.id === id);
      if (e && e.polyline.length >= 6) {
        const n = e.polyline.length / 3;
        const k = Math.floor((n - 1) / 2) * 3;
        p = new Vector3(
          (e.polyline[k]! + e.polyline[k + 3]!) / 2,
          (e.polyline[k + 1]! + e.polyline[k + 4]!) / 2,
          (e.polyline[k + 2]! + e.polyline[k + 5]!) / 2,
        );
      }
    }
    if (!p) return null;
    const q = p.applyMatrix4(this.root.matrixWorld).project(this.camera);
    const rect = this.renderer.domElement.getBoundingClientRect();
    return { x: rect.left + ((q.x + 1) / 2) * rect.width, y: rect.top + ((1 - q.y) / 2) * rect.height };
  }

  private disposePart(part: Part): void {
    if (this.transform.object === part.mesh) this.transform.detach();
    this.root.remove(part.mesh);
    part.mesh.geometry.dispose();
    part.mesh.material.dispose();
    part.edges.geometry.dispose();
    part.edges.material.dispose();
  }

  private applyStyles(): void {
    for (const part of this.parts.values()) {
      const selected = part.id === this.selected;
      const hovered = part.id === this.hovered && !selected;
      part.mesh.material.emissive.copy(selected ? ACCENT : hovered ? HOVER : new Color(0));
      part.mesh.material.emissiveIntensity = selected ? 0.45 : hovered ? 0.6 : 0;
      part.edges.material.color.copy(selected ? ACCENT : EDGE);
      part.edges.material.opacity = selected ? 1 : 0.55;
    }
  }

  private syncTransform(): void {
    const part = this.selected ? this.parts.get(this.selected) : undefined;
    if (this.tool === "move" && part && this.movable.has(part.id)) {
      if (this.transform.object !== part.mesh) this.transform.attach(part.mesh);
    } else if (this.transform.object) {
      this.transform.detach();
    }
  }

  private finishMove(): void {
    const mesh = this.transform.object as Mesh | undefined;
    if (!mesh) return;
    const part = this.parts.get(String(mesh.userData.id));
    if (!part) return;
    const delta = snapMove([mesh.position.x - part.center.x, mesh.position.y - part.center.y, 0]);
    // The engine decides the final position; snap back until it answers.
    mesh.position.copy(part.center);
    if (delta[0] !== 0 || delta[1] !== 0) this.cb.onMove(part.id, delta);
  }

  private pointer(event: PointerEvent): Vector2 {
    const rect = this.renderer.domElement.getBoundingClientRect();
    return new Vector2(((event.clientX - rect.left) / rect.width) * 2 - 1, -((event.clientY - rect.top) / rect.height) * 2 + 1);
  }

  private hit(event: PointerEvent) {
    this.raycaster.setFromCamera(this.pointer(event), this.camera as Camera);
    const hits = this.raycaster.intersectObjects([...this.parts.values()].map((p) => p.mesh), false);
    return hits[0] ?? null;
  }

  private onPointerDown = (event: PointerEvent): void => {
    if (event.button === 0) this.down = [event.clientX, event.clientY];
  };

  private onPointerUp = (event: PointerEvent): void => {
    if (event.button !== 0 || !this.down || this.dragging) return;
    const click = isClick(this.down, [event.clientX, event.clientY]);
    this.down = null;
    if (!click) return;
    if (this.tool === "measure") {
      const point = this.measureTarget(event);
      if (point) this.addMeasurePoint(point.point);
      return;
    }
    if (this.pickMode !== "part") {
      const found = this.topoAt(event);
      this.cb.onPick(found?.id ?? null, found?.topo ?? null, event.shiftKey);
      return;
    }
    const hit = this.hit(event);
    this.cb.onPick(hit ? String(hit.object.userData.id) : null, null, event.shiftKey);
  };

  private onPointerMove = (event: PointerEvent): void => {
    if (this.hoverFrame) return;
    this.hoverFrame = requestAnimationFrame(() => {
      this.hoverFrame = 0;
      if (this.dragging || (event.buttons & 1) === 1) return;
      if (this.tool === "measure") {
        const target = this.measureTarget(event);
        this.marker.visible = Boolean(target);
        if (target) {
          this.marker.position.set(...target.point);
          this.marker.material.color.set(target.snapped ? PALETTE.pick : PALETTE.text);
        }
        return;
      }
      if (this.pickMode !== "part") {
        const found = this.topoAt(event);
        this.cb.onHover(found?.id ?? null, found?.topo ?? null);
        return;
      }
      const hit = this.hit(event);
      const id = hit ? String(hit.object.userData.id) : null;
      if (id !== this.hovered) this.cb.onHover(id, null);
    });
  };

  private onPointerLeave = (): void => {
    this.marker.visible = false;
    if (this.hovered || this.hoverTopo) this.cb.onHover(null, null);
  };

  /** Surface hit, snapped to the nearest corner of the hit part within 14 px. */
  private measureTarget(event: PointerEvent): { point: Vec3; snapped: boolean } | null {
    const hit = this.hit(event);
    if (!hit) return null;
    const part = this.parts.get(String(hit.object.userData.id));
    const rect = this.renderer.domElement.getBoundingClientRect();
    if (part) {
      const candidates = part.corners.map((c) => {
        const p = new Vector3(...c).project(this.camera);
        return {
          value: c,
          screen: [rect.left + ((p.x + 1) / 2) * rect.width, rect.top + ((1 - p.y) / 2) * rect.height] as [number, number],
        };
      });
      const near = nearestScreenPoint([event.clientX, event.clientY], candidates, 14);
      if (near) return { point: near.value, snapped: true };
    }
    return { point: [hit.point.x, hit.point.y, hit.point.z], snapped: false };
  }

  private addMeasurePoint(point: Vec3): void {
    if (this.measurePoints.length >= 2) this.measurePoints = [];
    this.measurePoints.push(point);
    const pts = this.measurePoints.flat();
    this.measureLine.geometry.dispose();
    this.measureLine.geometry = new BufferGeometry();
    this.measureLine.geometry.setAttribute("position", new BufferAttribute(new Float32Array(pts.length === 3 ? [...pts, ...pts] : pts), 3));
    this.measureLine.visible = true;
    if (this.measurePoints.length === 2) {
      const [a, b] = this.measurePoints as [Vec3, Vec3];
      this.cb.onMeasure({ ...measure(a, b), a, b });
    } else {
      this.cb.onMeasure(null);
    }
  }

  private clearMeasure(): void {
    this.measurePoints = [];
    this.measureLine.visible = false;
    this.marker.visible = false;
    this.cb.onMeasure(null);
  }

  /** Projects a world point to host-relative pixels for HTML overlays. */
  project(point: Vec3): { x: number; y: number; visible: boolean } {
    const p = new Vector3(...point).project(this.camera);
    return {
      x: ((p.x + 1) / 2) * this.host.clientWidth,
      y: ((1 - p.y) / 2) * this.host.clientHeight,
      visible: p.z > -1 && p.z < 1,
    };
  }

  private resize(): void {
    const w = Math.max(1, this.host.clientWidth);
    const h = Math.max(1, this.host.clientHeight);
    this.renderer.setSize(w, h, false);
    this.renderer.domElement.style.width = `${w}px`;
    this.renderer.domElement.style.height = `${h}px`;
    this.persp.aspect = w / h;
    this.persp.updateProjectionMatrix();
    const half = (this.ortho.userData.halfHeight as number | undefined) ?? 100;
    this.ortho.left = (-half * w) / h;
    this.ortho.right = (half * w) / h;
    this.ortho.top = half;
    this.ortho.bottom = -half;
    this.ortho.updateProjectionMatrix();
  }

  private frame(): void {
    this.controls.update();
    const scale = this.camera === this.ortho
      ? ((this.ortho.top - this.ortho.bottom) / this.ortho.zoom) * 0.006
      : this.camera.position.distanceTo(this.marker.position) * 0.006;
    this.marker.scale.setScalar(Math.max(0.15, scale));
    const w = this.host.clientWidth;
    const h = this.host.clientHeight;
    this.renderer.setViewport(0, 0, w, h);
    this.renderer.setScissorTest(false);
    this.renderer.clear();
    this.renderer.render(this.scene, this.camera);
    // Axis triad, bottom-left.
    const size = 84;
    const dir = this.camera.position.clone().sub(this.controls.target).normalize().multiplyScalar(4);
    this.axesCamera.position.copy(dir);
    this.axesCamera.up.copy(this.camera.up);
    this.axesCamera.lookAt(0, 0, 0);
    this.renderer.setScissorTest(true);
    this.renderer.setScissor(8, 8, size, size);
    this.renderer.setViewport(8, 8, size, size);
    this.renderer.clearDepth();
    this.renderer.render(this.axesScene, this.axesCamera);
    this.renderer.setScissorTest(false);
  }
}

function disposeObject(o: Object3D): void {
  const m = o as Mesh;
  m.geometry?.dispose();
  const mat = m.material as { dispose?: () => void } | undefined;
  mat?.dispose?.();
}

/** Linear interpolation between the two frames around `t`. */
export function frameAt(sim: SimResult, t: number): SimFrame {
  const frames = sim.frames;
  const i = Math.max(0, Math.min(frames.length - 1, Math.floor(t * sim.fps)));
  const a = frames[i]!;
  const b = frames[Math.min(frames.length - 1, i + 1)]!;
  const span = b.t - a.t;
  const u = span > 0 ? Math.max(0, Math.min(1, (t - a.t) / span)) : 0;
  const mix = (x: number, y: number) => x + (y - x) * u;
  return {
    t,
    heave_in: mix(a.heave_in, b.heave_in),
    roll_deg: mix(a.roll_deg, b.roll_deg),
    pitch_deg: mix(a.pitch_deg, b.pitch_deg),
    min_freeboard_in: mix(a.min_freeboard_in, b.min_freeboard_in),
    walker: a.walker && b.walker ? [mix(a.walker[0], b.walker[0]), mix(a.walker[1], b.walker[1])] : a.walker,
  };
}

/** Same Airy wave as liquidcad-sim: deep water, ramped in over two periods. */
export function waveEta(p: NonNullable<SimResult["params"]>, x: number, y: number, t: number): number {
  const omega = (2 * Math.PI) / p.period_s;
  const k = ((omega * omega) / 9.80665) * 0.0254; // per inch
  const h = (p.heading_deg * Math.PI) / 180;
  const ramp = p.period_s * 2;
  const fade = t < ramp ? 0.5 - 0.5 * Math.cos((Math.PI * t) / ramp) : 1;
  return fade * (p.wave_height_in / 2) * Math.cos(k * (Math.sin(h) * x + Math.cos(h) * y) - omega * t);
}
