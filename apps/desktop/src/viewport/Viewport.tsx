import { useEffect, useRef, useState } from "react";
import { num } from "../format";
import { actions, getState, setState, useApp, type PickMode, type Tool, type ViewName } from "../store";
import { CadScene } from "./cadScene";
import type { Measurement, Vec3 } from "./snap";

declare global {
  interface Window {
    __liquidcadViewport?: {
      screenPoint: (id: string) => { x: number; y: number } | null;
      topoPoint: (component: string, kind: "face" | "edge", id: string) => { x: number; y: number } | null;
      meshCount: () => number;
      revision: () => number;
    };
  }
}

export function Viewport() {
  const host = useRef<HTMLDivElement>(null);
  const scene = useRef<CadScene | null>(null);
  const document = useApp((s) => s.session?.document ?? null);
  const selectedId = useApp((s) => s.selectedId);
  const hovered = useApp((s) => s.hovered);
  const tool = useApp((s) => s.tool);
  const view = useApp((s) => s.view);
  const ortho = useApp((s) => s.ortho);
  const fitNonce = useApp((s) => s.fitNonce);
  const fitAll = useApp((s) => s.fitAll);
  const pending = useApp((s) => s.pending);
  const pickMode = useApp((s) => s.pickMode);
  const picks = useApp((s) => s.picks);
  const hoverTopo = useApp((s) => s.hoverTopo);
  const preview = useApp((s) => s.preview);
  const showMass = useApp((s) => s.showMass);
  const workspace = useApp((s) => s.workspace);
  const sim = useApp((s) => s.sim);
  const simTime = useApp((s) => s.simTime);
  const selectedMovable = useApp((s) => s.session?.document.components.find((c) => c.id === s.selectedId)?.kind === "cleat");
  const [measurement, setMeasurement] = useState<(Measurement & { a: Vec3; b: Vec3 }) | null>(null);
  const [water, setWater] = useState(true);
  const [meshCount, setMeshCount] = useState(0);

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const cad = new CadScene(el, {
      onPick: (id, topo, additive) => {
        if (getState().pickMode === "part") return void actions.select(id);
        actions.pick(id && topo ? { component: id, ...topo } : null, additive);
      },
      onHover: (id, topo) => setState({ hovered: id, hoverTopo: id && topo ? { component: id, ...topo } : null }),
      onMove: (id, delta) => {
        const current = getPlacement(id);
        void actions.movePart(id, [current[0] + delta[0], current[1] + delta[1], 0]);
      },
      onMeasure: setMeasurement,
    });
    scene.current = cad;
    window.__liquidcadViewport = {
      screenPoint: (id) => cad.screenPoint(id),
      topoPoint: (component, kind, id) => cad.topoScreenPoint(component, kind, id),
      meshCount: () => cad.meshCount,
      revision: () => cad.revision,
    };
    return () => {
      delete window.__liquidcadViewport;
      cad.dispose();
      scene.current = null;
    };
  }, []);

  useEffect(() => {
    scene.current?.setDocument(document);
    setMeshCount(scene.current?.meshCount ?? 0);
  }, [document]);
  useEffect(() => scene.current?.setSelection(selectedId), [selectedId, document]);
  useEffect(() => scene.current?.setHover(hovered), [hovered]);
  useEffect(() => scene.current?.setTool(tool), [tool]);
  useEffect(() => scene.current?.setOrtho(ortho), [ortho]);
  useEffect(() => scene.current?.setWaterVisible(water), [water]);
  useEffect(() => scene.current?.setPickMode(pickMode), [pickMode]);
  useEffect(() => scene.current?.setPicks(picks), [picks, document]);
  useEffect(() => scene.current?.setHoverTopo(hoverTopo), [hoverTopo]);
  useEffect(() => scene.current?.setPreview(preview), [preview]);
  useEffect(() => scene.current?.setShowMass(showMass), [showMass, document]);
  useEffect(() => scene.current?.setSimulation(workspace === "simulate" ? sim : null, simTime), [workspace, sim, simTime, document]);
  useEffect(() => {
    if (view.nonce > 0) scene.current?.setView(view.name);
  }, [view]);
  useEffect(() => {
    if (fitNonce > 0) scene.current?.setView("iso");
  }, [fitNonce]);
  useEffect(() => {
    if (fitAll > 0) scene.current?.fit();
  }, [fitAll]);

  const setView = (name: ViewName) => setState((s) => ({ view: { name, nonce: s.view.nonce + 1 } }));
  const setTool = (next: Tool) => setState((s) => ({ tool: s.tool === next ? "select" : next }));
  const label = measurement ? scene.current?.project(midpoint(measurement.a, measurement.b)) : null;

  return (
    <div className="viewport">
      <div
        className="viewport-canvas"
        ref={host}
        data-testid="viewport"
        data-mesh-count={meshCount}
        data-revision={document?.revision ?? -1}
        data-tool={tool}
      />
      <div className="viewport-toolbar">
        <div className="seg">
          {(["iso", "top", "front", "right"] as const).map((name) => (
            <button key={name} onClick={() => setView(name)} title={`${name} view`}>
              {name === "iso" ? "Iso" : name[0]!.toUpperCase() + name.slice(1)}
            </button>
          ))}
          <button onClick={() => scene.current?.fit()} title="Fit all (F)">
            Fit
          </button>
        </div>
        <div className="seg">
          <button className={!ortho ? "on" : ""} onClick={() => setState({ ortho: false })}>
            Persp
          </button>
          <button className={ortho ? "on" : ""} onClick={() => setState({ ortho: true })}>
            Ortho
          </button>
        </div>
        <div className="seg">
          <button className={tool === "select" ? "on" : ""} onClick={() => setState({ tool: "select" })} title="Select (Esc)">
            Select
          </button>
          <button className={tool === "measure" ? "on" : ""} onClick={() => setTool("measure")} title="Measure, snaps to corners (M)">
            Measure
          </button>
          <button
            className={tool === "move" ? "on" : ""}
            onClick={() => setTool("move")}
            title="Move a cleat on the deck plane, 0.5 in snap (G)"
            disabled={!selectedMovable && tool !== "move"}
          >
            Move
          </button>
        </div>
        <div className="seg" data-testid="pick-mode">
          {(["part", "face", "edge"] as PickMode[]).map((mode) => (
            <button
              key={mode}
              className={pickMode === mode ? "on" : ""}
              onClick={() => setState({ pickMode: mode, picks: mode === "part" ? [] : getState().picks, tool: "select" })}
              title={
                mode === "part"
                  ? "Pick whole parts"
                  : `Pick ${mode}s: click to select, shift-click to add (fillets, holes, mates, measuring)`
              }
            >
              {mode[0]!.toUpperCase() + mode.slice(1)}
            </button>
          ))}
        </div>
        <div className="seg">
          <button className={water ? "on" : ""} onClick={() => setWater((w) => !w)} title="Show the at-rest waterline">
            Waterline
          </button>
          <button
            className={showMass ? "on" : ""}
            onClick={() => setState((s) => ({ showMass: !s.showMass }))}
            title="Centre of gravity (red) and centre of buoyancy (blue)"
            data-testid="toggle-mass"
          >
            CG/CB
          </button>
        </div>
      </div>
      {pending > 0 ? <div className="rebuilding">Regenerating...</div> : null}
      {!document ? (
        <div className="viewport-empty">
          <p>No project open.</p>
          <button className="primary" onClick={() => void actions.openSample()}>
            Open modular dock
          </button>
        </div>
      ) : null}
      {pickMode !== "part" && tool === "select" ? (
        <div className="viewport-hint">
          Click a {pickMode} to select it; shift-click to add more. {picks.length > 0 ? `${picks.length} picked.` : ""}
        </div>
      ) : null}
      {preview ? <div className="preview-badge">Preview: not yet applied</div> : null}
      {tool === "measure" ? (
        <div className="viewport-hint">Click two points. Corners snap within 14 px (green marker). Esc to finish.</div>
      ) : null}
      {tool === "move" ? (
        <div className="viewport-hint">
          {selectedMovable ? "Drag the arrows to slide the cleat along the deck. 0.5 in snap." : "Select a cleat to move it."}
        </div>
      ) : null}
      {measurement && label?.visible ? (
        <div className="measure-label" style={{ left: label.x, top: label.y }} data-testid="measure-label">
          {num(measurement.distance, 3)} in
          <small>
            dx {num(measurement.delta[0], 2)} dy {num(measurement.delta[1], 2)} dz {num(measurement.delta[2], 2)}
          </small>
        </div>
      ) : null}
    </div>
  );
}

function midpoint(a: Vec3, b: Vec3): Vec3 {
  return [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2, (a[2] + b[2]) / 2];
}

function getPlacement(id: string): [number, number, number] {
  return getState().session?.document.components.find((c) => c.id === id)?.placement ?? [0, 0, 0];
}
