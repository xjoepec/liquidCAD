import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent, type WheelEvent } from "react";
import { engine } from "../api";
import { actions, log, useApp } from "../store";
import type { Sheet, SheetPrim } from "../types";

export function DrawingWorkspace() {
  const revision = useApp((s) => s.session?.document.revision ?? null);
  const [sheet, setSheet] = useState<Sheet | null>(null);
  const [view, setView] = useState({ scale: 1, x: 0, y: 0 });
  const drag = useRef<{ x: number; y: number; vx: number; vy: number } | null>(null);

  useEffect(() => {
    if (revision === null) return;
    let live = true;
    engine
      .drawing()
      .then((s) => live && setSheet(s))
      .catch((err) => log("error", `drawing: ${err instanceof Error ? err.message : String(err)}`));
    return () => {
      live = false;
    };
  }, [revision]);

  if (revision === null) return <div className="workspace-empty">Open a project to generate its drawing sheet.</div>;
  if (!sheet) return <div className="workspace-empty">Generating drawing...</div>;

  const onWheel = (e: WheelEvent<HTMLDivElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    const factor = Math.exp(-e.deltaY * 0.0015);
    const scale = Math.min(8, Math.max(0.4, view.scale * factor));
    const cx = e.clientX - rect.left;
    const cy = e.clientY - rect.top;
    setView({ scale, x: cx - ((cx - view.x) * scale) / view.scale, y: cy - ((cy - view.y) * scale) / view.scale });
  };
  const onDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = { x: e.clientX, y: e.clientY, vx: view.x, vy: view.y };
  };
  const onMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (d) setView((v) => ({ ...v, x: d.vx + e.clientX - d.x, y: d.vy + e.clientY - d.y }));
  };

  return (
    <div className="drawing-workspace">
      <div className="workspace-toolbar">
        <span>
          {sheet.title} <small>ANSI B, {sheet.scale_label}, rev {revision}</small>
        </span>
        <div className="seg">
          <button onClick={() => setView({ scale: 1, x: 0, y: 0 })}>Fit</button>
          <button onClick={() => void actions.exportFile("pdf")}>Export PDF</button>
          <button onClick={() => void actions.exportFile("dxf")}>Export DXF</button>
        </div>
      </div>
      <div
        className="sheet-host"
        onWheel={onWheel}
        onPointerDown={onDown}
        onPointerMove={onMove}
        onPointerUp={() => (drag.current = null)}
      >
        <div className="sheet-paper" style={{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.scale})` }}>
          <SheetSvg sheet={sheet} />
        </div>
      </div>
    </div>
  );
}

const color = (c: [number, number, number]) => `rgb(${c.map((v) => Math.round(v * 255)).join(",")})`;

export function SheetSvg({ sheet }: { sheet: Sheet }) {
  const H = sheet.height;
  return (
    <svg viewBox={`0 0 ${sheet.width} ${sheet.height}`} className="sheet-svg" data-testid="drawing-sheet">
      <rect width={sheet.width} height={sheet.height} fill="oklch(1 0 0)" />
      {sheet.items.map((p, i) => (
        <Prim key={i} p={p} H={H} />
      ))}
    </svg>
  );
}

function Prim({ p, H }: { p: SheetPrim; H: number }) {
  switch (p.type) {
    case "line":
      return (
        <line
          x1={p.a[0]}
          y1={H - p.a[1]}
          x2={p.b[0]}
          y2={H - p.b[1]}
          stroke={color(p.color)}
          strokeWidth={p.weight}
          strokeDasharray={p.dash ? "4 2" : undefined}
          strokeLinecap="round"
        />
      );
    case "rect":
      return (
        <rect
          x={p.x}
          y={H - p.y - p.h}
          width={p.w}
          height={p.h}
          fill={p.fill ? color(p.fill) : "none"}
          stroke={p.stroke ? color(p.stroke) : "none"}
          strokeWidth={p.weight}
        />
      );
    case "text": {
      const x = p.at[0];
      const y = H - p.at[1];
      return (
        <text
          x={x}
          y={y}
          fontSize={p.size}
          fontFamily="Helvetica, Arial, sans-serif"
          fontWeight={p.bold ? 700 : 400}
          fill={color(p.color)}
          textAnchor={p.anchor}
          transform={p.rotate ? `rotate(${-p.rotate} ${x} ${y})` : undefined}
        >
          {p.text}
        </text>
      );
    }
  }
}
