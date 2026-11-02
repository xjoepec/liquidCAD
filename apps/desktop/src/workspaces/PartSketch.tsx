// Part sketcher: draw closed profiles on a plane, then extrude them into a
// part (or cut them into the part the sketch sits on). Snapping here is a
// drawing aid only; the engine validates every profile and builds the part.
import { useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent, type ReactElement, type WheelEvent as ReactWheelEvent } from "react";
import type { FeatureDraft } from "../../shared/ipc";
import { num, pounds } from "../format";
import { dot } from "../geometry";
import { actions, setState, useApp, type SketchTarget } from "../store";
import type { PathVertex, SessionState, SketchPlane, Vec3 } from "../types";
import { sketchSnap, type SnapKind } from "../viewport/snap";

type Tool = "line" | "rect" | "circle";
type Pt = [number, number];

const PLANES: { on: "xy" | "xz" | "yz"; label: string; hint: string }[] = [
  { on: "xy", label: "XY (plan)", hint: "Horizontal; offset is the height (z). Extrudes up." },
  { on: "xz", label: "XZ (front)", hint: "Vertical across the dock; offset is y. Extrudes toward the stern." },
  { on: "yz", label: "YZ (side)", hint: "Vertical along the dock; offset is x. Extrudes toward starboard." },
];

/** Sketch frame of a face, mirroring liquidcad_core::generators::plane_frame. */
function faceFrame(centroid: Vec3, normal: Vec3) {
  const up: Vec3 = Math.abs(normal[2]) < 0.9 ? [0, 0, 1] : [0, 1, 0];
  const k = dot(up, normal);
  const v0: Vec3 = [up[0] - normal[0] * k, up[1] - normal[1] * k, up[2] - normal[2] * k];
  const len = Math.hypot(...v0) || 1;
  const v: Vec3 = [v0[0] / len, v0[1] / len, v0[2] / len];
  const u: Vec3 = [v[1] * normal[2] - v[2] * normal[1], v[2] * normal[0] - v[0] * normal[2], v[0] * normal[1] - v[1] * normal[0]];
  return { origin: centroid, u, v };
}

function loopToPath(loop: PathVertex[]): PathVertex[] {
  return loop.map((p) => (p.bulge ? { x: p.x, y: p.y, bulge: p.bulge } : { x: p.x, y: p.y }));
}

/** SVG path for a loop, arcs from bulges. Sketch y is up; SVG y is down. */
function svgPath(loop: PathVertex[], closed = true): string {
  if (loop.length === 0) return "";
  let d = `M ${loop[0]!.x} ${-loop[0]!.y}`;
  const n = loop.length;
  for (let i = 0; i < (closed ? n : n - 1); i++) {
    const a = loop[i]!;
    const b = loop[(i + 1) % n]!;
    if (a.bulge) {
      const sweep = 4 * Math.atan(a.bulge);
      const chord = Math.hypot(b.x - a.x, b.y - a.y);
      const r = chord / (2 * Math.abs(Math.sin(sweep / 2)));
      // Positive bulge is counter-clockwise in sketch space, which is
      // clockwise once y flips for SVG.
      d += ` A ${r} ${r} 0 ${Math.abs(sweep) > Math.PI ? 1 : 0} ${a.bulge > 0 ? 0 : 1} ${b.x} ${-b.y}`;
    } else {
      d += ` L ${b.x} ${-b.y}`;
    }
  }
  return closed ? `${d} Z` : d;
}

export function PartSketch({ session, target }: { session: SessionState; target: SketchTarget }) {
  const engine = useApp((s) => s.engine);
  const preview = useApp((s) => s.preview);
  const native = engine.state === "ready" && engine.nativeOcct;
  const editing = target.featureId ?? null;
  const [plane, setPlane] = useState<SketchPlane>(target.plane);
  const [loops, setLoops] = useState<PathVertex[][]>(() => {
    const p = target.spec?.profile;
    return p ? [p.outer, ...(p.holes ?? [])] : [];
  });
  const [name, setName] = useState(target.name ?? "Bracket");
  const [depth, setDepth] = useState(String(target.spec?.depth ?? 0.5));
  const [direction, setDirection] = useState(target.spec?.direction ?? "forward");
  const [material, setMaterial] = useState(target.material ?? "alum_6061");
  const [cut, setCut] = useState(Boolean(target.spec?.cut_into));
  const [tool, setTool] = useState<Tool>("rect");
  const [grid, setGrid] = useState(0.25);
  const [drawing, setDrawing] = useState<Pt[]>([]);
  const [cursor, setCursor] = useState<{ p: Pt; kind: SnapKind } | null>(null);
  const [view, setView] = useState({ cx: 0, cy: 0, half: 30 });
  const svgRef = useRef<SVGSVGElement>(null);
  const pan = useRef<{ x: number; y: number; cx: number; cy: number } | null>(null);

  // Reference: the face the sketch sits on, projected into sketch space.
  const reference = useMemo(() => {
    if (plane.on !== "face") return null;
    const mesh = session.document.scene.meshes.find((m) => m.component_id === plane.component);
    const face = mesh?.faces.find((f) => f.id === plane.face);
    if (!mesh || !face) return null;
    const frame = faceFrame(face.centroid, face.normal);
    const tris: Pt[][] = [];
    for (let t = face.tri_start; t < face.tri_start + face.tri_count; t++) {
      tris.push(
        [0, 1, 2].map((k) => {
          const i = mesh.indices[t * 3 + k]! * 3;
          const d: Vec3 = [
            mesh.positions[i]! - frame.origin[0],
            mesh.positions[i + 1]! - frame.origin[1],
            mesh.positions[i + 2]! - frame.origin[2],
          ];
          return [dot(d, frame.u), dot(d, frame.v)] as Pt;
        }),
      );
    }
    return { tris, face };
  }, [plane, session.document.scene.meshes]);

  // Frame the reference (or the existing profile) once.
  useEffect(() => {
    const pts: Pt[] = reference ? reference.tris.flat() : loops.flat().map((p) => [p.x, p.y] as Pt);
    if (pts.length === 0) return;
    const xs = pts.map((p) => p[0]);
    const ys = pts.map((p) => p[1]);
    const [x0, x1, y0, y1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
    setView({ cx: (x0 + x1) / 2, cy: (y0 + y1) / 2, half: Math.max(6, Math.max(x1 - x0, y1 - y0) * 0.65) });
    // Only when the plane changes.
  }, [reference === null, plane.on === "face" ? `${plane.component}/${plane.face}` : plane.on]);

  const depthValue = Number(depth);
  const draft: FeatureDraft | null = useMemo(() => {
    if (loops.length === 0 || !(depthValue > 0)) return null;
    return {
      name: name.trim() || "Sketched part",
      material,
      spec: {
        type: "extrude",
        plane,
        profile: { outer: loopToPath(loops[0]!), holes: loops.slice(1).map(loopToPath) },
        depth: depthValue,
        direction,
        cut_into: cut && plane.on === "face" ? plane.component : null,
      },
    };
  }, [loops, depthValue, name, material, plane, direction, cut]);

  // Debounced engine preview.
  useEffect(() => {
    const t = setTimeout(() => void actions.preview(draft), 250);
    return () => clearTimeout(t);
  }, [JSON.stringify(draft)]);
  useEffect(() => () => void actions.preview(null), []);

  const pxPerUnit = () => {
    const rect = svgRef.current?.getBoundingClientRect();
    return rect ? Math.min(rect.width, rect.height) / (view.half * 2) : 10;
  };
  const toSketch = (event: { clientX: number; clientY: number }): Pt => {
    const svg = svgRef.current!;
    const pt = svg.createSVGPoint();
    pt.x = event.clientX;
    pt.y = event.clientY;
    const p = pt.matrixTransform(svg.getScreenCTM()!.inverse());
    return [p.x, -p.y];
  };
  const snapAt = (event: { clientX: number; clientY: number }) =>
    sketchSnap(toSketch(event), {
      vertices: loops.flat().map((p) => [p.x, p.y] as Pt),
      segments: loops.flatMap((l) => l.map((p, i) => [[p.x, p.y], [l[(i + 1) % l.length]!.x, l[(i + 1) % l.length]!.y]] as [Pt, Pt])),
      drawing,
      grid,
      tolerance: 10 / pxPerUnit(),
    });

  const commitLoop = (pts: PathVertex[]) => {
    if (pts.length < 2) return;
    setLoops((ls) => [...ls, pts]);
    setDrawing([]);
  };

  const onDown = (event: ReactPointerEvent<SVGSVGElement>) => {
    if (event.button === 1 || event.button === 2 || event.altKey) {
      pan.current = { x: event.clientX, y: event.clientY, cx: view.cx, cy: view.cy };
      event.currentTarget.setPointerCapture(event.pointerId);
      return;
    }
    if (event.button !== 0) return;
    const { p, kind } = snapAt(event);
    if (tool === "line") {
      if (drawing.length >= 3 && kind === "close") {
        commitLoop(drawing.map(([x, y]) => ({ x, y })));
        return;
      }
      setDrawing((d) => [...d, p]);
    } else if (tool === "rect") {
      if (drawing.length === 0) setDrawing([p]);
      else {
        const [a] = drawing as [Pt];
        if (Math.abs(p[0] - a[0]) > 1e-6 && Math.abs(p[1] - a[1]) > 1e-6) {
          commitLoop([
            { x: a[0], y: a[1] },
            { x: p[0], y: a[1] },
            { x: p[0], y: p[1] },
            { x: a[0], y: p[1] },
          ]);
        }
      }
    } else if (tool === "circle") {
      if (drawing.length === 0) setDrawing([p]);
      else {
        const [c] = drawing as [Pt];
        const r = Math.hypot(p[0] - c[0], p[1] - c[1]);
        if (r > 1e-3) {
          commitLoop([
            { x: c[0] - r, y: c[1], bulge: 1 },
            { x: c[0] + r, y: c[1], bulge: 1 },
          ]);
        }
      }
    }
  };
  const onMove = (event: ReactPointerEvent<SVGSVGElement>) => {
    if (pan.current) {
      const k = pxPerUnit();
      setView((v) => ({ ...v, cx: pan.current!.cx - (event.clientX - pan.current!.x) / k, cy: pan.current!.cy + (event.clientY - pan.current!.y) / k }));
      return;
    }
    setCursor(snapAt(event));
  };
  const onUp = () => {
    pan.current = null;
  };
  const onWheel = (event: ReactWheelEvent) => {
    const f = Math.exp(event.deltaY * 0.0015);
    setView((v) => ({ ...v, half: Math.min(2000, Math.max(0.5, v.half * f)) }));
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const typing = document.activeElement instanceof HTMLInputElement || document.activeElement instanceof HTMLSelectElement;
      if (typing) return;
      if (e.key === "Escape") setDrawing([]);
      if (e.key === "Enter" && tool === "line" && drawing.length >= 3) commitLoop(drawing.map(([x, y]) => ({ x, y })));
      if (e.key === "Backspace" && drawing.length > 0) setDrawing((d) => d.slice(0, -1));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [tool, drawing]);

  const close = () => {
    void actions.preview(null);
    setState({ sketchTarget: null, workspace: "model" });
  };
  const submit = async () => {
    if (!draft) return;
    const ok = editing ? await actions.editFeature(editing, draft) : await actions.addFeature(draft);
    if (ok) close();
  };

  const setVertex = (li: number, vi: number, axis: "x" | "y", raw: string) => {
    const v = Number(raw);
    if (!Number.isFinite(v)) return;
    setLoops((ls) => ls.map((l, i) => (i !== li ? l : l.map((p, j) => (j === vi ? { ...p, [axis]: v } : p)))));
  };

  const k = 1;
  const vb = `${view.cx - view.half} ${-view.cy - view.half} ${view.half * 2} ${view.half * 2}`;
  const stroke = (px: number) => px / pxPerUnit();
  const gridStep = (() => {
    const target = view.half / 10;
    const steps = [0.125, 0.25, 0.5, 1, 2, 6, 12, 24, 48, 96];
    return steps.find((s) => s >= target) ?? 96;
  })();
  const gridLines: ReactElement[] = [];
  const x0 = Math.floor((view.cx - view.half) / gridStep) * gridStep;
  const y0 = Math.floor((view.cy - view.half) / gridStep) * gridStep;
  for (let x = x0; x <= view.cx + view.half; x += gridStep) {
    gridLines.push(<line key={`gx${x}`} x1={x} y1={-(view.cy + view.half)} x2={x} y2={-(view.cy - view.half)} className="sk-grid" strokeWidth={stroke(1)} />);
  }
  for (let y = y0; y <= view.cy + view.half; y += gridStep) {
    gridLines.push(<line key={`gy${y}`} x1={view.cx - view.half} y1={-y} x2={view.cx + view.half} y2={-y} className="sk-grid" strokeWidth={stroke(1)} />);
  }
  const ghost: PathVertex[] = (() => {
    if (!cursor || drawing.length === 0) return [];
    const p = cursor.p;
    if (tool === "line") return [...drawing, p].map(([x, y]) => ({ x, y }));
    const a = drawing[0]!;
    if (tool === "rect")
      return [
        { x: a[0], y: a[1] },
        { x: p[0], y: a[1] },
        { x: p[0], y: p[1] },
        { x: a[0], y: p[1] },
      ];
    const r = Math.hypot(p[0] - a[0], p[1] - a[1]);
    return r > 0 ? [{ x: a[0] - r, y: a[1], bulge: 1 }, { x: a[0] + r, y: a[1], bulge: 1 }] : [];
  })();
  const materials = Object.entries(session.document.project.materials);
  const problems = preview?.diagnostics ?? [];

  return (
    <div className="part-sketch" data-testid="part-sketch">
      <div className="sk-toolbar">
        <div className="seg">
          {(["line", "rect", "circle"] as Tool[]).map((t) => (
            <button
              key={t}
              className={tool === t ? "on" : ""}
              disabled={t === "circle" && !native}
              title={t === "circle" && !native ? "Circles and arcs need OpenCascade, which this build does not link" : `${t} tool`}
              onClick={() => {
                setTool(t);
                setDrawing([]);
              }}
              data-testid={`sk-tool-${t}`}
            >
              {t === "rect" ? "Rectangle" : t[0]!.toUpperCase() + t.slice(1)}
            </button>
          ))}
        </div>
        <label className="inline">
          snap
          <select value={grid} onChange={(e) => setGrid(Number(e.target.value))}>
            {[0.0625, 0.125, 0.25, 0.5, 1, 6].map((g) => (
              <option key={g} value={g}>
                {g} in
              </option>
            ))}
          </select>
        </label>
        <button onClick={() => setLoops((l) => l.slice(0, -1))} disabled={loops.length === 0}>
          Undo loop
        </button>
        <button onClick={() => setLoops([])} disabled={loops.length === 0}>
          Clear
        </button>
        <span className="sk-status">
          {cursor ? `${num(cursor.p[0], 3)}, ${num(cursor.p[1], 3)} in ${cursor.kind !== "free" ? `(${cursor.kind})` : ""}` : ""}
        </span>
      </div>
      <div className="sk-body">
        <svg
          ref={svgRef}
          className="sk-canvas"
          viewBox={vb}
          preserveAspectRatio="xMidYMid meet"
          onPointerDown={onDown}
          onPointerMove={onMove}
          onPointerUp={onUp}
          onPointerLeave={() => setCursor(null)}
          onWheel={onWheel}
          onContextMenu={(e) => e.preventDefault()}
          data-testid="sketch-canvas"
        >
          {gridLines}
          <line x1={view.cx - view.half} y1={0} x2={view.cx + view.half} y2={0} className="sk-axis-x" strokeWidth={stroke(1.5)} />
          <line x1={0} y1={-(view.cy - view.half)} x2={0} y2={-(view.cy + view.half)} className="sk-axis-y" strokeWidth={stroke(1.5)} />
          {reference?.tris.map((t, i) => (
            <polygon key={i} points={t.map((p) => `${p[0]},${-p[1]}`).join(" ")} className="sk-reference" />
          ))}
          {loops.map((l, i) => (
            <path key={i} d={svgPath(l)} className={i === 0 ? "sk-outer" : "sk-hole"} strokeWidth={stroke(2)} />
          ))}
          {loops.flat().map((p, i) => (
            <circle key={i} cx={p.x} cy={-p.y} r={stroke(3) * k} className="sk-vertex" />
          ))}
          {ghost.length > 0 ? <path d={svgPath(ghost, tool !== "line")} className="sk-ghost" strokeWidth={stroke(1.5)} /> : null}
          {cursor ? (
            <circle cx={cursor.p[0]} cy={-cursor.p[1]} r={stroke(5)} className={`sk-snap ${cursor.kind}`} strokeWidth={stroke(1.5)} />
          ) : null}
        </svg>
        <aside className="sk-panel">
          <h2>{editing ? "Edit sketched part" : "New sketched part"}</h2>
          <div className="field">
            <label>Plane</label>
            <div className="seg wrap">
              {PLANES.map((p) => (
                <button
                  key={p.on}
                  className={plane.on === p.on ? "on" : ""}
                  title={p.hint}
                  onClick={() =>
                    setPlane({ on: p.on, offset: plane.on === p.on && "offset" in plane ? plane.offset : 0 } as SketchPlane)
                  }
                >
                  {p.label}
                </button>
              ))}
              {target.plane.on === "face" ? (
                <button className={plane.on === "face" ? "on" : ""} onClick={() => setPlane(target.plane)}>
                  Face {target.plane.face}
                </button>
              ) : null}
            </div>
            {plane.on !== "face" ? (
              <div className="input-row">
                <span className="unit">offset</span>
                <input
                  value={String(plane.offset)}
                  onChange={(e) => Number.isFinite(Number(e.target.value)) && setPlane({ ...plane, offset: Number(e.target.value) })}
                />
                <span className="unit">in</span>
              </div>
            ) : (
              <p className="hint">
                On {plane.face} of {plane.component}. Sketch y follows world up where it can.
              </p>
            )}
          </div>
          <div className="field">
            <label htmlFor="sk-name">Name</label>
            <input id="sk-name" value={name} onChange={(e) => setName(e.target.value)} data-testid="sk-name" />
          </div>
          <div className="input-row">
            <span className="unit">depth</span>
            <input value={depth} onChange={(e) => setDepth(e.target.value)} data-testid="sk-depth" />
            <span className="unit">in</span>
            <select value={direction} onChange={(e) => setDirection(e.target.value as typeof direction)}>
              <option value="forward">out</option>
              <option value="backward">in</option>
              <option value="symmetric">both ways</option>
            </select>
          </div>
          {plane.on === "face" ? (
            <label className="check">
              <input type="checkbox" checked={cut} onChange={(e) => setCut(e.target.checked)} disabled={!native} />
              Cut into {plane.component} {native ? "" : "(needs OpenCascade)"}
            </label>
          ) : null}
          {!cut ? (
            <div className="field">
              <label>Material</label>
              <select value={material} onChange={(e) => setMaterial(e.target.value)}>
                {materials.map(([key, m]) => (
                  <option key={key} value={key}>
                    {m.name}
                  </option>
                ))}
              </select>
            </div>
          ) : null}
          <h3>Profile</h3>
          {loops.length === 0 ? (
            <p className="hint">Draw a closed outline. Later loops inside it become holes. Alt-drag or right-drag pans; wheel zooms.</p>
          ) : null}
          <div className="sk-loops">
            {loops.map((l, li) => (
              <div key={li} className="sk-loop">
                <div className="sk-loop-head">
                  {li === 0 ? "Outline" : `Hole ${li}`}
                  <button className="link danger" onClick={() => setLoops((ls) => ls.filter((_, i) => i !== li))}>
                    remove
                  </button>
                </div>
                {l.map((p, vi) => (
                  <div key={vi} className="input-row compact">
                    <input value={String(p.x)} onChange={(e) => setVertex(li, vi, "x", e.target.value)} aria-label="x" />
                    <input value={String(p.y)} onChange={(e) => setVertex(li, vi, "y", e.target.value)} aria-label="y" />
                    {p.bulge ? <span className="unit">arc</span> : null}
                  </div>
                ))}
              </div>
            ))}
          </div>
          <div className="sk-preview" data-testid="sk-preview">
            {preview?.mass ? (
              <p>
                {pounds(preview.mass.mass_lb)} · {num(preview.mass.volume_in3, 2)} in³
              </p>
            ) : null}
            {problems.map((d, i) => (
              <p key={i} className={d.severity === "error" ? "error" : "hint"}>
                {d.message}
              </p>
            ))}
          </div>
          <div className="row-buttons">
            <button className="primary" disabled={!draft || problems.some((d) => d.severity === "error")} onClick={() => void submit()} data-testid="sk-create">
              {editing ? "Update part" : cut ? "Cut" : "Create part"}
            </button>
            <button onClick={close}>Cancel</button>
          </div>
        </aside>
      </div>
    </div>
  );
}
