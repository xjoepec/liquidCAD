import { useEffect, useState, type KeyboardEvent } from "react";
import type { MateInput } from "../../shared/ipc";
import { checkParameter, inches, num, pounds, rgb, unitSuffix } from "../format";
import { angleDeg, dot, norm, sub, toLocal } from "../geometry";
import { actions, setState, useApp, type TopoPick } from "../store";
import type { Component, EdgeInfo, FaceInfo, MassProps, Mesh, Parameter, SessionState, Vec3 } from "../types";

export function Inspector() {
  const session = useApp((s) => s.session);
  return (
    <aside className="panel right" data-testid="inspector">
      {session ? (
        <>
          <Selection session={session} />
          <Picks session={session} />
          <Parameters session={session} />
          <Engineering session={session} />
        </>
      ) : (
        <>
          <h2>Inspector</h2>
          <p className="empty">Parameters, dimensions, and materials appear once a project is open.</p>
        </>
      )}
    </aside>
  );
}

const GROUP_LABEL: Record<string, string> = {
  plan: "Plan",
  floats: "Floats",
  frame: "Frame",
  deck: "Deck",
  loads: "Loads",
  gangway: "Gangway",
};

function Parameters({ session }: { session: SessionState }) {
  const params = Object.values(session.document.project.parameters);
  const groups = [...new Set(params.map((p) => p.group))];
  return (
    <section>
      <h2>Parameters</h2>
      {groups.map((group) => (
        <div key={group} className="param-group">
          <h3>{GROUP_LABEL[group] ?? group}</h3>
          {params
            .filter((p) => p.group === group)
            .map((p) => (
              <ParameterField key={p.name} parameter={p} />
            ))}
        </div>
      ))}
    </section>
  );
}

export function ParameterField({ parameter: p }: { parameter: Parameter }) {
  const engineError = useApp((s) => s.paramErrors[p.name] ?? "");
  const [text, setText] = useState(num(p.value, 3));
  const [localError, setLocalError] = useState<string | null>(null);
  useEffect(() => {
    setText(num(p.value, 3));
    setLocalError(null);
  }, [p.value]);
  const fixed = p.min === p.max;

  const commit = (raw: string) => {
    const value = Number(raw);
    const problem = checkParameter(p, value);
    setLocalError(problem);
    if (problem || Math.abs(value - p.value) < 1e-9) return;
    void actions.setParameter(p.name, value);
  };
  const step = (dir: number) => {
    const next = Math.min(p.max, Math.max(p.min, p.value + dir * p.step));
    if (next !== p.value) void actions.setParameter(p.name, next);
  };
  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") commit(text);
    if (e.key === "Escape") {
      setText(num(p.value, 3));
      setLocalError(null);
    }
    if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      step(e.key === "ArrowUp" ? 1 : -1);
    }
  };
  const error = localError ?? (engineError || null);
  return (
    <div className={`field ${error ? "invalid" : ""}`}>
      <label htmlFor={`param-${p.name}`}>
        {p.label}
        <small>
          {num(p.min)}-{num(p.max)} {p.snap ? `step ${num(p.step)}` : ""}
        </small>
      </label>
      <div className="input-row">
        <button className="step" onClick={() => step(-1)} disabled={fixed || p.value <= p.min} aria-label={`decrease ${p.label}`}>
          -
        </button>
        <input
          id={`param-${p.name}`}
          data-testid={`param-${p.name}`}
          value={text}
          inputMode="decimal"
          disabled={fixed}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={onKey}
          onBlur={() => commit(text)}
        />
        <span className="unit">{unitSuffix(p)}</span>
        <button className="step" onClick={() => step(1)} disabled={fixed || p.value >= p.max} aria-label={`increase ${p.label}`}>
          +
        </button>
      </div>
      {error ? <p className="error" data-testid={`param-error-${p.name}`}>{error}</p> : null}
    </div>
  );
}

function Selection({ session }: { session: SessionState }) {
  const selection = useApp((s) => s.selection);
  const selectedId = useApp((s) => s.selectedId);
  if (!selectedId) {
    return (
      <section>
        <h2>Selection</h2>
        <p className="empty">Click a part in the viewport or assembly tree.</p>
      </section>
    );
  }
  if (!selection || selection.component.id !== selectedId) {
    return (
      <section>
        <h2>Selection</h2>
        <p className="empty">Loading {selectedId}...</p>
      </section>
    );
  }
  const c = selection.component;
  const project = session.document.project;
  const feature = project.features.find((f) => f.id === c.feature_id);
  const options = Object.entries(project.materials).filter(
    ([, m]) => feature && (m.applies_to.length === 0 || m.applies_to.includes(feature.kind.type)),
  );
  const [sx, sy, sz] = c.bounds.size;
  const userFeature = feature?.spec ? feature : null;
  const modifiers = project.features.filter(
    (f) => f.spec?.type === "modify" && f.spec.target === c.id,
  );
  return (
    <section className="selection" data-testid="selection">
      <h2>
        Selection <button className="link" onClick={() => void actions.select(null)}>clear</button>
      </h2>
      <div className="sel-title">
        <strong>{c.name}</strong>
        <code>{c.id}</code>
      </div>
      <dl className="props">
        <dt>Kind</dt>
        <dd>{c.kind.replace("_", " ")}</dd>
        <dt>Feature</dt>
        <dd>{selection.feature_name}</dd>
        <dt>Extents</dt>
        <dd className="mono">
          {num(sx)} x {num(sy)} x {num(sz)} in
        </dd>
        <dt>Origin</dt>
        <dd className="mono">{c.bounds.origin.map((v) => num(v)).join(", ")}</dd>
        <dt>Shape</dt>
        <dd>{shapeLabel(c)}</dd>
        {c.stock ? (
          <>
            <dt>Stock</dt>
            <dd>
              {c.stock.profile}, cut {inches(c.stock.cut_length_in)}
            </dd>
          </>
        ) : null}
        <dt>Mass</dt>
        <dd data-testid="part-mass">
          {num(c.mass.mass_lb, 3)} lb <small>({num(c.mass.volume_in3, 1)} in3 envelope)</small>
        </dd>
        <dt>Material</dt>
        <dd>
          <span className="swatch" style={{ background: rgb(selection.material.color) }} />
          {feature && options.length > 1 ? (
            <select
              data-testid="material-select"
              value={c.material}
              onChange={(e) => void actions.setMaterial(feature.id, e.target.value)}
            >
              {options.map(([key, m]) => (
                <option key={key} value={key}>
                  {m.name}
                </option>
              ))}
            </select>
          ) : (
            selection.material.name
          )}
        </dd>
        <dt>Density</dt>
        <dd>
          {num(selection.material.density_lb_ft3)} lb/ft3
          {selection.material.max_span_in ? `, span ${num(selection.material.max_span_in)} in max` : ""}
        </dd>
      </dl>
      <MassTable mass={c.mass} />
      {selection.movable ? <Placement id={c.id} placement={c.placement} /> : null}
      {userFeature ? (
        <div className="user-feature">
          <h3>{userFeature.name}</h3>
          <div className="row-buttons">
            {userFeature.spec?.type === "extrude" ? (
              <button
                onClick={() => {
                  const spec = userFeature.spec;
                  if (spec?.type !== "extrude") return;
                  setState({
                    workspace: "sketch",
                    sketchTarget: {
                      plane: spec.plane,
                      featureId: userFeature.id,
                      name: userFeature.name,
                      material: userFeature.material,
                      spec,
                    },
                  });
                }}
              >
                Edit sketch
              </button>
            ) : null}
            <button className="danger" onClick={() => void actions.deleteFeature(userFeature.id)}>
              Delete feature
            </button>
          </div>
        </div>
      ) : null}
      {modifiers.length > 0 ? (
        <div className="user-feature">
          <h3>Modifiers</h3>
          {modifiers.map((f) => (
            <div key={f.id} className="modifier-row">
              <span>{f.name}</span>
              <button className="link danger" onClick={() => void actions.deleteFeature(f.id)} data-testid={`delete-${f.id}`}>
                delete
              </button>
            </div>
          ))}
        </div>
      ) : null}
      <div className="chips">
        <span className="chip-label">Driven by</span>
        {selection.driven_by.map((name) => (
          <button
            key={name}
            className="chip"
            onClick={() => document.getElementById(`param-${name}`)?.focus()}
            title="Focus this parameter"
          >
            {project.parameters[name]?.label ?? name}
          </button>
        ))}
      </div>
    </section>
  );
}

function Placement({ id, placement }: { id: string; placement: [number, number, number] }) {
  const [dx, setDx] = useState(num(placement[0]));
  const [dy, setDy] = useState(num(placement[1]));
  useEffect(() => {
    setDx(num(placement[0]));
    setDy(num(placement[1]));
  }, [placement]);
  const apply = () => {
    const x = Number(dx);
    const y = Number(dy);
    if (Number.isFinite(x) && Number.isFinite(y)) void actions.movePart(id, [x, y, 0]);
  };
  return (
    <div className="placement">
      <h3>Placement offset</h3>
      <div className="input-row">
        <span className="unit">dx</span>
        <input data-testid="place-dx" value={dx} onChange={(e) => setDx(e.target.value)} onKeyDown={(e) => e.key === "Enter" && apply()} />
        <span className="unit">dy</span>
        <input data-testid="place-dy" value={dy} onChange={(e) => setDy(e.target.value)} onKeyDown={(e) => e.key === "Enter" && apply()} />
      </div>
      <div className="row-buttons">
        <button onClick={apply}>Apply</button>
        <button onClick={() => void actions.movePart(id, [0, 0, 0])}>Reset</button>
        <button onClick={() => setState({ tool: "move", workspace: "model" })}>Move tool</button>
      </div>
    </div>
  );
}

function Engineering({ session }: { session: SessionState }) {
  const e = session.document.engineering;
  const reserve = Math.max(0, Math.min(1, e.reserve_buoyancy_ratio));
  const tone = e.reserve_buoyancy_ratio < 0 ? "bad" : e.reserve_buoyancy_ratio < 0.25 ? "warn" : "good";
  return (
    <section data-testid="engineering">
      <h2>Hydrostatics</h2>
      <div className="meter">
        <div className="meter-label">
          Reserve buoyancy <strong>{Math.round(e.reserve_buoyancy_ratio * 100)}%</strong>
        </div>
        <div className="meter-track">
          <div className={`meter-fill ${tone}`} style={{ width: `${reserve * 100}%` }} />
        </div>
      </div>
      <dl className="props">
        <dt>Displacement</dt>
        <dd>{pounds(e.buoyancy_capacity_lb)}</dd>
        <dt>Dead load</dt>
        <dd>{pounds(e.dead_load_lb)}</dd>
        <dt>Live load</dt>
        <dd>
          {pounds(e.live_load_lb)} <small>on {num(e.deck_area_ft2, 1)} ft2</small>
        </dd>
        <dt>Draft</dt>
        <dd>
          {inches(e.draft_dead_in, 1)} rest, {inches(e.draft_loaded_in, 1)} loaded
        </dd>
        <dt>Freeboard</dt>
        <dd>
          {inches(e.freeboard_dead_in, 1)} rest, {inches(e.freeboard_loaded_in, 1)} loaded
        </dd>
        <dt>CG</dt>
        <dd className="mono">{e.center_of_gravity.map((v) => num(v, 1)).join(", ")}</dd>
        <dt>CB</dt>
        <dd className="mono">{e.center_of_buoyancy.map((v) => num(v, 1)).join(", ")}</dd>
      </dl>
      <h2>Stability</h2>
      <dl className="props" data-testid="stability">
        <dt>GM across</dt>
        <dd className={e.gm_transverse_in <= 0 ? "bad" : ""}>{inches(e.gm_transverse_in, 1)}</dd>
        <dt>GM along</dt>
        <dd>{inches(e.gm_longitudinal_in, 0)}</dd>
        <dt>Static heel</dt>
        <dd>{num(e.heel_static_deg, 2)} deg</dd>
        <dt>Static trim</dt>
        <dd data-testid="trim">{num(e.trim_static_deg, 2)} deg</dd>
        <dt>Crowd heel</dt>
        <dd>
          {num(Math.abs(e.heel_crowd_deg), 1)} deg <small>half live load on port half, edge {inches(e.edge_freeboard_crowd_in, 1)}</small>
        </dd>
        <dt>Assembly</dt>
        <dd>
          {pounds(e.mass.mass_lb)} <small>incl. gangway; floats carry {pounds(e.floating.mass_lb)}</small>
        </dd>
      </dl>
    </section>
  );
}

function shapeLabel(c: Component): string {
  const base = c.shape.type === "modified" ? c.shape.base : c.shape;
  const ops = c.shape.type === "modified" ? c.shape.ops.length : 0;
  const what =
    base.type === "box"
      ? "box"
      : base.type === "prism"
        ? (base.profile.holes?.length ?? 0) > 0
          ? `prism with ${base.profile.holes!.length} hole loop(s)`
          : "prism"
        : base.type === "revolve"
          ? "revolve"
          : "shape";
  return ops > 0 ? `${what} + ${ops} modifier(s)` : what;
}

function MassTable({ mass }: { mass: MassProps }) {
  return (
    <details className="mass" open>
      <summary>Mass properties</summary>
      <dl className="props">
        <dt>CG</dt>
        <dd className="mono">{mass.com.map((v) => num(v, 2)).join(", ")} in</dd>
        <dt>Principal I</dt>
        <dd className="mono">{mass.principal.map((v) => num(v, 0)).join(", ")} lb·in²</dd>
        <dt>Ixx Iyy Izz</dt>
        <dd className="mono">{[0, 1, 2].map((i) => num(mass.inertia[i]![i]!, 0)).join(", ")}</dd>
      </dl>
    </details>
  );
}

type Resolved = { pick: TopoPick; component: Component; mesh: Mesh; face?: FaceInfo; edge?: EdgeInfo };

function resolve(session: SessionState, pick: TopoPick): Resolved | null {
  const component = session.document.components.find((c) => c.id === pick.component);
  const mesh = session.document.scene.meshes.find((m) => m.component_id === pick.component);
  if (!component || !mesh) return null;
  if (pick.kind === "face") {
    const face = mesh.faces.find((f) => f.id === pick.id);
    return face ? { pick, component, mesh, face } : null;
  }
  const edge = mesh.edge_info.find((e) => e.id === pick.id);
  return edge ? { pick, component, mesh, edge } : null;
}

function edgeDir(e: EdgeInfo): Vec3 {
  const p = e.polyline;
  const n = p.length;
  return [p[n - 3]! - p[0]!, p[n - 2]! - p[1]!, p[n - 1]! - p[2]!];
}

function Picks({ session }: { session: SessionState }) {
  const picks = useApp((s) => s.picks);
  const engine = useApp((s) => s.engine);
  const native = engine.state === "ready" && engine.nativeOcct;
  const resolved = picks.map((p) => resolve(session, p)).filter((r): r is Resolved => r !== null);
  if (resolved.length === 0) return null;
  const [a, b] = resolved;
  let relation: string | null = null;
  if (a?.face && b?.face && resolved.length === 2) {
    const angle = angleDeg(a.face.normal, b.face.normal);
    if (angle < 0.01 || angle > 179.99) {
      const gap = Math.abs(dot(sub(b.face.centroid, a.face.centroid), a.face.normal));
      relation = `parallel, ${num(gap, 3)} in apart`;
    } else {
      relation = `${num(angle, 2)} deg between normals`;
    }
  } else if (a?.edge && b?.edge && resolved.length === 2) {
    relation = `${num(angleDeg(edgeDir(a.edge), edgeDir(b.edge)), 2)} deg between edges`;
  }
  const edges = resolved.filter((r) => r.edge);
  const sameComponentEdges = edges.length > 0 && edges.length === resolved.length && edges.every((r) => r.component.id === edges[0]!.component.id);
  const singleFace = resolved.length === 1 && a?.face ? a : null;
  const mateable = resolved.length === 2 && a?.edge && b?.edge && a.component.feature_id !== b.component.feature_id;
  const needs = native ? undefined : "Needs OpenCascade, which this build does not link";
  return (
    <section data-testid="picks">
      <h2>
        Picked {resolved.length === 1 ? (a!.face ? "face" : "edge") : `${resolved.length} items`}{" "}
        <button className="link" onClick={() => setState({ picks: [] })}>
          clear
        </button>
      </h2>
      <ul className="pick-list">
        {resolved.map((r) => (
          <li key={`${r.pick.component}/${r.pick.kind}/${r.pick.id}`} data-testid="pick-item">
            <code>{r.pick.id}</code> <small>on {r.component.name}</small>
            {r.face ? (
              <div className="mono small">
                {r.face.kind}, {num(r.face.area_in2, 3)} in², normal ({r.face.normal.map((v) => num(v, 3)).join(", ")})
              </div>
            ) : null}
            {r.edge ? (
              <div className="mono small">
                {r.edge.kind}, {num(r.edge.length_in, 3)} in
              </div>
            ) : null}
          </li>
        ))}
      </ul>
      {relation ? (
        <p className="relation" data-testid="pick-relation">
          {relation}
        </p>
      ) : null}
      {sameComponentEdges ? <EdgeOps target={edges[0]!.component} edges={edges.map((e) => e.pick.id)} disabledReason={needs} /> : null}
      {singleFace ? <FaceOps pick={singleFace} disabledReason={needs} /> : null}
      {mateable ? <MateForm a={a!} b={b!} session={session} /> : null}
    </section>
  );
}

function EdgeOps({ target, edges, disabledReason }: { target: Component; edges: string[]; disabledReason?: string }) {
  const [size, setSize] = useState("0.25");
  const value = Number(size);
  const valid = Number.isFinite(value) && value > 0;
  const add = (op: "fillet" | "chamfer") => {
    const draft = {
      name: `${op === "fillet" ? "Fillet" : "Chamfer"} ${size} in on ${target.name}`,
      spec: {
        type: "modify" as const,
        target: target.id,
        op: op === "fillet" ? { op, edges, radius: value } : { op, edges, distance: value },
      },
    };
    void actions.addFeature(draft).then((ok) => ok && setState({ picks: [] }));
  };
  return (
    <div className="ops">
      <h3>Edge treatment</h3>
      <div className="input-row">
        <span className="unit">size</span>
        <input value={size} onChange={(e) => setSize(e.target.value)} data-testid="edge-size" />
        <span className="unit">in</span>
      </div>
      <div className="row-buttons">
        <button disabled={!valid || Boolean(disabledReason)} title={disabledReason} onClick={() => add("fillet")} data-testid="add-fillet">
          Fillet
        </button>
        <button disabled={!valid || Boolean(disabledReason)} title={disabledReason} onClick={() => add("chamfer")}>
          Chamfer
        </button>
      </div>
      {disabledReason ? <p className="hint">{disabledReason}.</p> : null}
    </div>
  );
}

function FaceOps({ pick, disabledReason }: { pick: Resolved; disabledReason?: string }) {
  const [dia, setDia] = useState("0.5");
  const [depth, setDepth] = useState("0");
  const d = Number(dia);
  const h = Number(depth);
  const planar = pick.face?.kind === "plane";
  const valid = Number.isFinite(d) && d > 0 && Number.isFinite(h) && h >= 0;
  const drill = () => {
    const at = toLocal(pick.component.pose, pick.pick.point);
    void actions
      .addFeature({
        name: `Hole D${dia} in ${pick.component.name}`,
        spec: { type: "modify", target: pick.component.id, op: { op: "hole", face: pick.pick.id, at, diameter: d, depth: h } },
      })
      .then((ok) => ok && setState({ picks: [] }));
  };
  return (
    <div className="ops">
      <h3>On this face</h3>
      <div className="row-buttons">
        <button
          disabled={!planar}
          title={planar ? "Sketch a new part on this face" : "Sketches need a flat face"}
          data-testid="sketch-on-face"
          onClick={() =>
            setState({
              workspace: "sketch",
              sketchTarget: { plane: { on: "face", component: pick.component.id, face: pick.pick.id } },
            })
          }
        >
          Sketch on face
        </button>
      </div>
      <div className="input-row">
        <span className="unit">dia</span>
        <input value={dia} onChange={(e) => setDia(e.target.value)} data-testid="hole-dia" />
        <span className="unit">depth</span>
        <input value={depth} onChange={(e) => setDepth(e.target.value)} title="0 drills through" />
      </div>
      <div className="row-buttons">
        <button
          disabled={!valid || !planar || Boolean(disabledReason)}
          title={disabledReason ?? "Drill at the point you clicked, square to the face"}
          onClick={drill}
          data-testid="drill-hole"
        >
          Drill hole here
        </button>
      </div>
    </div>
  );
}

function MateForm({ a, b, session }: { a: Resolved; b: Resolved; session: SessionState }) {
  const [kind, setKind] = useState<"revolute" | "fixed" | "prismatic">("revolute");
  const [value, setValue] = useState("0");
  const [parameter, setParameter] = useState("");
  const params = Object.values(session.document.project.parameters).filter((p) =>
    kind === "revolute" ? p.unit === "degree" : p.unit === "inch",
  );
  const lengthA = norm(edgeDir(a.edge!));
  const add = () => {
    const n = (session.document.project.mates.length + 1).toString();
    const mate: MateInput = {
      id: `mate.user.${n}.${Date.now().toString(36)}`,
      name: `${kind[0]!.toUpperCase()}${kind.slice(1)} ${b.component.name} to ${a.component.name}`,
      kind,
      parent: { component: a.component.id, edge: a.pick.id },
      child: { component: b.component.id, edge: b.pick.id },
      parameter: parameter || null,
      value: Number(value) || 0,
      invert: false,
      limits: null,
    };
    void actions.addMate(mate).then((ok) => ok && setState({ picks: [] }));
  };
  return (
    <div className="ops">
      <h3>Mate</h3>
      <p className="hint">
        {b.component.name}'s feature moves onto {a.component.name}'s edge ({num(lengthA, 1)} in).
      </p>
      <div className="input-row">
        <select value={kind} onChange={(e) => setKind(e.target.value as typeof kind)} data-testid="mate-kind">
          <option value="revolute">Hinge (revolute)</option>
          <option value="fixed">Fixed</option>
          <option value="prismatic">Slider (prismatic)</option>
        </select>
      </div>
      <div className="input-row">
        <select value={parameter} onChange={(e) => setParameter(e.target.value)}>
          <option value="">constant</option>
          {params.map((p) => (
            <option key={p.name} value={p.name}>
              {p.label}
            </option>
          ))}
        </select>
        {parameter ? null : (
          <>
            <input value={value} onChange={(e) => setValue(e.target.value)} />
            <span className="unit">{kind === "revolute" ? "deg" : "in"}</span>
          </>
        )}
      </div>
      <div className="row-buttons">
        <button onClick={add} data-testid="add-mate">
          Add mate
        </button>
      </div>
    </div>
  );
}
