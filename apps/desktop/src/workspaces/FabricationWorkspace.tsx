import { useEffect, useState } from "react";
import { engine } from "../api";
import { inches, num, pounds } from "../format";
import { actions, log, useApp } from "../store";
import type { CutPlan } from "../types";

export function FabricationWorkspace() {
  const session = useApp((s) => s.session);
  const revision = session?.document.revision ?? null;
  const [plan, setPlan] = useState<CutPlan | null>(null);

  useEffect(() => {
    if (revision === null) return;
    let live = true;
    engine
      .cutPlan()
      .then((p) => live && setPlan(p))
      .catch((err) => log("error", `cut plan: ${err instanceof Error ? err.message : String(err)}`));
    return () => {
      live = false;
    };
  }, [revision]);

  if (!session) return <div className="workspace-empty">Open a project to see its bill of materials.</div>;
  const { bom, diagnostics, engineering } = session.document;
  const totalQty = bom.reduce((s, l) => s + l.qty, 0);
  const totalWeight = bom.reduce((s, l) => s + l.total_weight_lb, 0);
  const mfg = diagnostics.filter((d) => d.source === "manufacturing");

  return (
    <div className="fab-workspace">
      <div className="workspace-toolbar">
        <span>
          Fabrication <small>rev {session.document.revision}</small>
        </span>
        <div className="seg">
          <button onClick={() => void actions.exportFile("csv")}>BOM CSV</button>
          <button onClick={() => void actions.exportFile("dxf")}>DXF</button>
          <button onClick={() => void actions.exportFile("step")}>STEP</button>
          <button onClick={() => void actions.exportFile("pdf")}>PDF</button>
          <button className="primary" onClick={() => void actions.exportAll()}>
            Export all...
          </button>
        </div>
      </div>
      <div className="fab-grid">
        <section className="card">
          <h2>Bill of materials</h2>
          <table className="grid" data-testid="bom">
            <thead>
              <tr>
                <th>#</th>
                <th>Item</th>
                <th>Material</th>
                <th>Size</th>
                <th>Cut</th>
                <th className="r">Qty</th>
                <th className="r">Each</th>
                <th className="r">Total</th>
              </tr>
            </thead>
            <tbody>
              {bom.map((line, i) => (
                <tr key={`${line.item}-${line.size}-${line.cut_length_in}`} data-kind={line.kind}>
                  <td className="mono">{i + 1}</td>
                  <td>{line.item}</td>
                  <td>{line.material}</td>
                  <td>{line.size}</td>
                  <td className="mono">{line.cut_length_in ? inches(line.cut_length_in) : ""}</td>
                  <td className="r mono" data-testid={`bom-qty-${line.kind}`}>
                    {line.qty}
                  </td>
                  <td className="r mono">{num(line.unit_weight_lb, 1)}</td>
                  <td className="r mono">{num(line.total_weight_lb, 1)}</td>
                </tr>
              ))}
            </tbody>
            <tfoot>
              <tr>
                <td />
                <td colSpan={4}>Total</td>
                <td className="r mono">{totalQty}</td>
                <td />
                <td className="r mono">{pounds(totalWeight)}</td>
              </tr>
            </tfoot>
          </table>
          <p className="hint">
            {engineering.float_count} floats carry {pounds(engineering.dead_load_lb)} dead and {pounds(engineering.live_load_lb)} live load.
          </p>
        </section>
        <section className="card">
          <h2>Stock cut plan</h2>
          {plan ? (
            plan.groups.map((g) => (
              <div key={`${g.material}-${g.profile}`} className="stock-group">
                <h3>
                  {g.profile} <small>{g.material}</small>
                </h3>
                <p className="meta">
                  {g.cut_count} cuts, {g.bars.length} x {g.stock_length_in ? inches(g.stock_length_in, 0) : "?"} stock,{" "}
                  {num(g.waste_ratio * 100, 0)}% offcut{g.splices ? `, ${g.splices} splice(s)` : ""}
                </p>
                <div className="bars">
                  {g.bars.slice(0, 24).map((bar, i) => (
                    <div key={i} className="bar" title={bar.cuts.map((c) => inches(c)).join(" + ")}>
                      {bar.cuts.map((cut, j) => (
                        <span key={j} className="cut" style={{ width: `${(cut / (g.stock_length_in ?? cut)) * 100}%` }}>
                          {cut >= (g.stock_length_in ?? 0) * 0.18 ? num(cut, 1) : ""}
                        </span>
                      ))}
                    </div>
                  ))}
                  {g.bars.length > 24 ? <p className="meta">+{g.bars.length - 24} more bars</p> : null}
                </div>
              </div>
            ))
          ) : (
            <p className="empty">Planning...</p>
          )}
          <h2>Manufacturing checks</h2>
          <ul className="diagnostics compact">
            {mfg.map((d, i) => (
              <li key={i} className={d.severity}>
                <span className={`sev ${d.severity}`}>{d.severity}</span>
                <code>{d.code}</code>
                <span className="msg">{d.message}</span>
              </li>
            ))}
          </ul>
        </section>
      </div>
    </div>
  );
}
