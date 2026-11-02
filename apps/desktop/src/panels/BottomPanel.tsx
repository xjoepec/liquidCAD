import { useState } from "react";
import { bridge } from "../api";
import { actions, setState, useApp } from "../store";
import type { BomLine, Diagnostic } from "../types";

const NO_DIAGNOSTICS: Diagnostic[] = [];
const NO_BOM: BomLine[] = [];

type Tab = "diagnostics" | "bom" | "history" | "output";

export function BottomPanel() {
  const [tab, setTab] = useState<Tab>("diagnostics");
  const diagnostics = useApp((s) => s.session?.document.diagnostics ?? NO_DIAGNOSTICS);
  const history = useApp((s) => s.history);
  const logCount = useApp((s) => s.log.filter((l) => l.level === "error").length);
  const counts = {
    error: diagnostics.filter((d) => d.severity === "error").length,
    warning: diagnostics.filter((d) => d.severity === "warning").length,
  };
  return (
    <section className="bottom" data-testid="bottom-panel">
      <nav className="tabs">
        <button className={tab === "diagnostics" ? "on" : ""} onClick={() => setTab("diagnostics")} data-testid="tab-diagnostics">
          Constraints &amp; diagnostics
          {counts.error ? <span className="badge error">{counts.error}</span> : null}
          {counts.warning ? <span className="badge warning">{counts.warning}</span> : null}
        </button>
        <button className={tab === "bom" ? "on" : ""} onClick={() => setTab("bom")} data-testid="tab-bom">
          Bill of materials
        </button>
        <button className={tab === "history" ? "on" : ""} onClick={() => setTab("history")} data-testid="tab-history">
          Revision history <span className="badge">{history.length}</span>
        </button>
        <button className={tab === "output" ? "on" : ""} onClick={() => setTab("output")} data-testid="tab-output">
          Output {logCount ? <span className="badge error">{logCount}</span> : null}
        </button>
      </nav>
      <div className="tab-body">
        {tab === "diagnostics" ? <Diagnostics items={diagnostics} /> : null}
        {tab === "bom" ? <BomSummary /> : null}
        {tab === "history" ? <History /> : null}
        {tab === "output" ? <Output /> : null}
      </div>
    </section>
  );
}

const ORDER = { error: 0, warning: 1, info: 2 } as const;

function Diagnostics({ items }: { items: Diagnostic[] }) {
  const components = useApp((s) => s.session?.document.components);
  const sorted = [...items].sort((a, b) => ORDER[a.severity] - ORDER[b.severity]);
  if (!components) return <p className="empty">No project open.</p>;
  return (
    <ul className="diagnostics" data-testid="diagnostics">
      {sorted.map((d, i) => {
        const target = d.related_ids.find((id) => components.some((c) => c.id === id));
        return (
          <li key={`${d.code}-${i}`} className={d.severity}>
            <span className={`sev ${d.severity}`}>{d.severity}</span>
            <code>{d.code}</code>
            <span className="msg">{d.message}</span>
            <span className="src">{d.source}</span>
            {target ? (
              <button
                className="link"
                onClick={() => {
                  setState({ workspace: "model" });
                  void actions.select(target);
                }}
              >
                show
              </button>
            ) : null}
          </li>
        );
      })}
    </ul>
  );
}

function BomSummary() {
  const bom = useApp((s) => s.session?.document.bom ?? NO_BOM);
  return (
    <table className="grid" data-testid="bom-summary">
      <thead>
        <tr>
          <th>Item</th>
          <th>Size</th>
          <th>Cut</th>
          <th className="r">Qty</th>
          <th className="r">Weight</th>
        </tr>
      </thead>
      <tbody>
        {bom.map((line) => (
          <tr key={`${line.item}-${line.size}-${line.cut_length_in}`}>
            <td>{line.item}</td>
            <td>{line.size}</td>
            <td className="mono">{line.cut_length_in ? `${line.cut_length_in} in` : ""}</td>
            <td className="r mono" data-testid={`bom-summary-qty-${line.kind}`}>
              {line.qty}
            </td>
            <td className="r mono">{line.total_weight_lb.toFixed(1)} lb</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function History() {
  const history = useApp((s) => s.history);
  if (history.length === 0) return <p className="empty">Edits are recorded in the local SQLite history.</p>;
  return (
    <table className="grid history" data-testid="history">
      <thead>
        <tr>
          <th>Rev</th>
          <th>Change</th>
          <th>When</th>
          <th>Parts</th>
          <th>Mass</th>
          <th />
        </tr>
      </thead>
      <tbody>
        {history.map((r, i) => (
          <tr key={r.id}>
            <td className="mono">{r.id}</td>
            <td>{r.note}</td>
            <td>{new Date(r.created_at).toLocaleTimeString()}</td>
            <td className="mono">{r.component_count}</td>
            <td className="mono">{Math.round(r.mass_lb)} lb</td>
            <td>{i > 0 ? <button className="link" onClick={() => void actions.restore(r.id)}>restore</button> : <small>current</small>}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function Output() {
  const log = useApp((s) => s.log);
  if (log.length === 0) return <p className="empty">Exports and errors appear here.</p>;
  return (
    <ul className="log" data-testid="output-log">
      {log.map((entry) => (
        <li key={entry.id} className={entry.level}>
          <time>{entry.at.toLocaleTimeString()}</time>
          <span>{entry.text}</span>
          {entry.path ? (
            <button className="link" onClick={() => void bridge().reveal(entry.path!)}>
              show in folder
            </button>
          ) : null}
        </li>
      ))}
    </ul>
  );
}
