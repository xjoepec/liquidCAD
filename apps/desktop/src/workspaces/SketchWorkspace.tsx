import { useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { feetInches, inches, num, snapParameter } from "../format";
import { ParameterField } from "../panels/Inspector";
import { actions, setState, useApp } from "../store";
import { PartSketch } from "./PartSketch";
import type { Parameter, Sketch } from "../types";

type Drag = { param: "width_in" | "length_in"; value: number } | null;

/** Sketch x (across) maps to screen -Y, sketch y (along) to screen X: bow right, port at the bottom. */
const toSvg = ([x, y]: [number, number]): [number, number] => [y, -x];

export function SketchWorkspace() {
  const session = useApp((s) => s.session);
  const target = useApp((s) => s.sketchTarget);
  if (session && target) {
    const key = target.featureId ?? (target.plane.on === "face" ? `${target.plane.component}/${target.plane.face}` : target.plane.on);
    return <PartSketch key={key} session={session} target={target} />;
  }
  return <PlanSketch />;
}

function PlanSketch() {
  const session = useApp((s) => s.session);
  const svgRef = useRef<SVGSVGElement>(null);
  const [drag, setDrag] = useState<Drag>(null);
  if (!session) return <div className="workspace-empty">Open a project to edit its plan sketch.</div>;
  const sketch = session.document.sketches[0];
  const params = session.document.project.parameters;
  const width = params.width_in!;
  const length = params.length_in!;
  const module = params.module_in!;
  if (!sketch) return <div className="workspace-empty">The plan sketch feature is suppressed.</div>;

  const W = drag?.param === "width_in" ? drag.value : width.value;
  const L = drag?.param === "length_in" ? drag.value : length.value;
  const pad = 70;
  const viewBox = `${-pad} ${-(width.max + pad)} ${length.max + pad * 2} ${width.max + pad * 2}`;

  const svgPoint = (event: ReactPointerEvent): [number, number] => {
    const svg = svgRef.current!;
    const pt = svg.createSVGPoint();
    pt.x = event.clientX;
    pt.y = event.clientY;
    const p = pt.matrixTransform(svg.getScreenCTM()!.inverse());
    return [p.x, p.y];
  };
  const start = (param: "width_in" | "length_in") => (event: ReactPointerEvent<SVGRectElement>) => {
    event.currentTarget.setPointerCapture(event.pointerId);
    setDrag({ param, value: param === "width_in" ? width.value : length.value });
  };
  const move = (event: ReactPointerEvent<SVGRectElement>) => {
    if (!drag) return;
    const [sx, sy] = svgPoint(event);
    const p: Parameter = drag.param === "width_in" ? width : length;
    const raw = drag.param === "width_in" ? -sy : sx;
    setDrag({ param: drag.param, value: snapParameter(p, raw) });
  };
  const end = () => {
    if (!drag) return;
    const current = drag.param === "width_in" ? width.value : length.value;
    if (drag.value !== current) void actions.setParameter(drag.param, drag.value);
    setDrag(null);
  };

  const cols = Math.round(W / module.value);
  const rows = Math.round(L / module.value);
  const line = (a: [number, number], b: [number, number]) => {
    const [x1, y1] = toSvg(a);
    const [x2, y2] = toSvg(b);
    return { x1, y1, x2, y2 };
  };

  return (
    <div className="sketch-workspace">
      <svg ref={svgRef} className="sketch" viewBox={viewBox} data-testid="sketch">
        <defs>
          <pattern id="minor" width="12" height="12" patternUnits="userSpaceOnUse">
            <path d="M 12 0 L 0 0 0 12" fill="none" style={{ stroke: "var(--rule)" }} strokeWidth="0.5" />
          </pattern>
        </defs>
        <rect x={-pad} y={-(width.max + pad)} width={length.max + pad * 2} height={width.max + pad * 2} fill="url(#minor)" />
        <rect className="envelope" x={0} y={-width.max} width={length.max} height={width.max} />
        <rect className="envelope min" x={0} y={-width.min} width={length.min} height={width.min} />
        <text className="envelope-label" x={length.max} y={-width.max - 6} textAnchor="end">
          max {feetInches(width.max)} x {feetInches(length.max)}
        </text>
        <text className="envelope-label" x={length.min} y={-width.min - 4} textAnchor="end">
          min
        </text>
        {Array.from({ length: Math.max(0, cols - 1) }, (_, i) => (
          <line key={`gx${i}`} className="construction" {...line([(i + 1) * module.value, 0], [(i + 1) * module.value, L])} />
        ))}
        {Array.from({ length: Math.max(0, rows - 1) }, (_, i) => (
          <line key={`gy${i}`} className="construction" {...line([0, (i + 1) * module.value], [W, (i + 1) * module.value])} />
        ))}
        <rect className={`profile ${drag ? "dragging" : ""}`} x={0} y={-W} width={L} height={W} />
        {drag ? null : <SketchEdges sketch={sketch} />}
        <Dimension a={[0, 0]} b={[0, L]} offset={[-26, 0]} label={`${inches(L, 0)}  ${feetInches(L)}`} active={drag?.param === "length_in"} />
        <Dimension a={[0, 0]} b={[W, 0]} offset={[0, -26]} label={`${inches(W, 0)}  ${feetInches(W)}`} active={drag?.param === "width_in"} />
        <Dimension
          a={[W, L - module.value]}
          b={[W, L]}
          offset={[16, 0]}
          label={inches(module.value, 0)}
          active={false}
        />
        <circle className="origin" cx={0} cy={0} r={3} />
        <text className="glyph-text" x={6} y={14}>
          fixed origin
        </text>
        <rect
          className="handle"
          data-testid="handle-width"
          x={L / 2 - 7}
          y={-W - 7}
          width={14}
          height={14}
          onPointerDown={start("width_in")}
          onPointerMove={move}
          onPointerUp={end}
          onPointerCancel={() => setDrag(null)}
        />
        <rect
          className="handle ew"
          data-testid="handle-length"
          x={L - 7}
          y={-W / 2 - 7}
          width={14}
          height={14}
          onPointerDown={start("length_in")}
          onPointerMove={move}
          onPointerUp={end}
          onPointerCancel={() => setDrag(null)}
        />
        <text className="axis-label" x={L + 14} y={-W / 2 + 4}>
          BOW
        </text>
        <text className="axis-label" x={L / 2} y={30} textAnchor="middle">
          PORT
        </text>
      </svg>
      <aside className="sketch-side">
        <h2>Part sketch</h2>
        <p className="hint">Sketch a new part (bracket, plate, ladder mount) on a principal plane, or pick a face in the model and choose Sketch on face.</p>
        <div className="row-buttons">
          <button
            className="primary"
            data-testid="new-part-sketch"
            onClick={() =>
              setState({ sketchTarget: { plane: { on: "xy", offset: session.document.engineering.deck_top_in } } })
            }
          >
            New part on deck plane
          </button>
        </div>
        <h2>Driving dimensions</h2>
        <ParameterField parameter={width} />
        <ParameterField parameter={length} />
        <ParameterField parameter={module} />
        <p className="hint">
          Drag the square handles on the starboard and bow edges. Edges snap to the {num(module.value)} in float module and
          stay inside the parameter envelope. Release to regenerate.
        </p>
        <h2>Constraints</h2>
        <ul className="constraints">
          {sketch.constraints.map((c) => (
            <li key={c.kind}>
              <span className="glyph-chip">{c.kind.replace("_", " ")}</span>
              <span>{c.note || c.entities.join(", ")}</span>
            </li>
          ))}
          <li>
            <span className="glyph-chip">grid</span>
            <span>
              {cols} x {rows} cells, fully constrained
            </span>
          </li>
        </ul>
      </aside>
    </div>
  );
}

function SketchEdges({ sketch }: { sketch: Sketch }) {
  return (
    <>
      {sketch.entities
        .filter((e) => !e.construction)
        .map((e) => {
          const [x1, y1] = toSvg(e.a);
          const [x2, y2] = toSvg(e.b);
          return <line key={e.id} className="edge" x1={x1} y1={y1} x2={x2} y2={y2} />;
        })}
    </>
  );
}

function Dimension({
  a,
  b,
  offset,
  label,
  active,
}: {
  a: [number, number];
  b: [number, number];
  offset: [number, number];
  label: string;
  active: boolean;
}) {
  const [ax, ay] = toSvg(a);
  const [bx, by] = toSvg(b);
  const [ox, oy] = offset;
  const vertical = Math.abs(ax - bx) < 1e-6;
  const mx = (ax + bx) / 2 + ox;
  const my = (ay + by) / 2 + oy;
  return (
    <g className={`dimension ${active ? "active" : ""}`}>
      <line x1={ax} y1={ay} x2={ax + ox} y2={ay + oy} className="ext" />
      <line x1={bx} y1={by} x2={bx + ox} y2={by + oy} className="ext" />
      <line x1={ax + ox} y1={ay + oy} x2={bx + ox} y2={by + oy} markerStart="" />
      <path d={`M ${ax + ox - 3} ${ay + oy + 3} L ${ax + ox + 3} ${ay + oy - 3} M ${bx + ox - 3} ${by + oy + 3} L ${bx + ox + 3} ${by + oy - 3}`} />
      <text
        x={vertical ? mx - 6 : mx}
        y={vertical ? my : my - 6}
        textAnchor="middle"
        transform={vertical ? `rotate(-90 ${mx - 6} ${my})` : undefined}
      >
        {label}
      </text>
    </g>
  );
}
