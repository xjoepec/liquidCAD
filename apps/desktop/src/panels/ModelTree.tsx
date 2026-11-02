import { useMemo, useState } from "react";
import { actions, setState, useApp } from "../store";
import type { Component, Feature } from "../types";

const KIND_GLYPH: Record<string, string> = {
  parameters: "P",
  plan_sketch: "S",
  float_pattern: "F",
  frame_beams: "B",
  deck_boards: "D",
  cleats: "C",
  bumpers: "R",
  gangway: "G",
  extrude: "E",
  modify: "M",
  assembly: "A",
};

export function ModelTree() {
  const session = useApp((s) => s.session);
  const regenerated = useApp((s) => s.regenerated);
  const selectedId = useApp((s) => s.selectedId);
  const hovered = useApp((s) => s.hovered);
  const [filter, setFilter] = useState("");
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({
    "feat.deck": true,
    "feat.floats": true,
    "feat.gangway": true,
  });

  const groups = useMemo(() => {
    const doc = session?.document;
    if (!doc) return [];
    const needle = filter.trim().toLowerCase();
    return doc.project.features
      .filter((f) => doc.components.some((c) => c.feature_id === f.id) || needle === "")
      .map((feature) => ({
        feature,
        parts: doc.components.filter(
          (c) => c.feature_id === feature.id && (!needle || c.name.toLowerCase().includes(needle) || c.id.includes(needle)),
        ),
      }))
      .filter((g) => g.parts.length > 0);
  }, [session, filter]);

  if (!session) {
    return (
      <aside className="panel left">
        <h2>Feature history</h2>
        <p className="empty">Open a project to see its feature graph.</p>
      </aside>
    );
  }
  const { document } = session;

  return (
    <aside className="panel left" data-testid="assembly-tree">
      <section>
        <h2>
          Feature history <small>{document.project.features.length}</small>
        </h2>
        <ol className="features">
          {document.project.features.map((feature) => (
            <FeatureRow
              key={feature.id}
              feature={feature}
              regenerated={regenerated.includes(feature.id)}
              partCount={document.components.filter((c) => c.feature_id === feature.id).length}
              materialName={feature.material ? document.project.materials[feature.material]?.name : undefined}
            />
          ))}
        </ol>
        <p className="legend">
          <span className="dot regen" /> regenerated <span className="dot reused" /> reused in {session.rebuild.elapsed_ms.toFixed(1)} ms
        </p>
      </section>
      {document.project.mates.length > 0 ? (
        <section data-testid="mates">
          <h2>
            Mates <small>{document.project.mates.length}</small>
          </h2>
          <ul className="mates">
            {document.project.mates.map((m) => {
              const solved = document.mates.find((s) => s.id === m.id);
              const unit = m.kind === "revolute" ? "deg" : "in";
              return (
                <li key={m.id} className={solved ? "" : "broken"} title={`${m.parent.component} ${m.parent.edge} <- ${m.child.component} ${m.child.edge}`}>
                  <span className="glyph">{m.kind === "revolute" ? "R" : m.kind === "prismatic" ? "P" : "F"}</span>
                  <span className="name">{m.name}</span>
                  <span className="mono small">{solved ? `${solved.value.toFixed(1)} ${unit}` : "unsolved"}</span>
                  {m.parameter ? (
                    <button className="link" onClick={() => document && window.document.getElementById(`param-${m.parameter}`)?.focus()}>
                      edit
                    </button>
                  ) : null}
                  <button className="link danger" onClick={() => void actions.deleteMate(m.id)} title="Delete mate">
                    x
                  </button>
                </li>
              );
            })}
          </ul>
        </section>
      ) : null}
      <section className="grow">
        <h2>
          Assembly <small>{document.components.length} parts</small>
        </h2>
        <input className="search" placeholder="Filter parts" value={filter} onChange={(e) => setFilter(e.target.value)} />
        <div className="tree">
          {groups.map(({ feature, parts }) => {
            const open = filter !== "" || !collapsed[feature.id];
            return (
              <div key={feature.id} className="group">
                <button className="group-head" onClick={() => setCollapsed((c) => ({ ...c, [feature.id]: !c[feature.id] }))}>
                  <span className="caret">{open ? "v" : ">"}</span> {feature.name} <small>{parts.length}</small>
                </button>
                {open
                  ? parts.map((part) => (
                      <PartRow key={part.id} part={part} selected={selectedId === part.id} hovered={hovered === part.id} />
                    ))
                  : null}
              </div>
            );
          })}
        </div>
      </section>
    </aside>
  );
}

function FeatureRow({
  feature,
  regenerated,
  partCount,
  materialName,
}: {
  feature: Feature;
  regenerated: boolean;
  partCount: number;
  materialName?: string;
}) {
  const solid = Boolean(feature.material);
  return (
    <li className={`feature ${feature.suppressed ? "suppressed" : ""}`} title={`${feature.id}\ninputs: ${feature.inputs.join(", ") || "none"}`}>
      <span className={`glyph k-${feature.kind.type}`}>{KIND_GLYPH[feature.kind.type] ?? "?"}</span>
      <span className="name">
        {feature.name}
        <small>{materialName ?? (feature.inputs.length ? `${feature.inputs.length} inputs` : feature.kind.type.replace("_", " "))}</small>
      </span>
      {partCount > 0 ? <span className="count">{partCount}</span> : null}
      <span className={`dot ${regenerated ? "regen" : "reused"}`} />
      {solid ? (
        <button
          className="icon"
          title={feature.suppressed ? "Unsuppress" : "Suppress"}
          onClick={() => void actions.setSuppressed(feature.id, !feature.suppressed)}
        >
          {feature.suppressed ? "off" : "on"}
        </button>
      ) : null}
    </li>
  );
}

function PartRow({ part, selected, hovered }: { part: Component; selected: boolean; hovered: boolean }) {
  return (
    <button
      className={`part ${selected ? "selected" : ""} ${hovered ? "hovered" : ""}`}
      data-id={part.id}
      onClick={() => void actions.select(selected ? null : part.id)}
      onMouseEnter={() => setState({ hovered: part.id })}
      onMouseLeave={() => setState({ hovered: null })}
    >
      <span>{part.name}</span>
      <code>{part.id}</code>
    </button>
  );
}
