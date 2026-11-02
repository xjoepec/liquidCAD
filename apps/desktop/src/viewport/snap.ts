// Pure geometry helpers for viewport interaction, kept free of Three.js so
// they are unit-testable.

export type Vec3 = [number, number, number];
export type Vec2 = [number, number];

/** Nearest candidate within `radiusPx` of `cursor` in screen space. */
export function nearestScreenPoint<T>(
  cursor: Vec2,
  candidates: { screen: Vec2; value: T }[],
  radiusPx: number,
): { value: T; distancePx: number } | null {
  let best: { value: T; distancePx: number } | null = null;
  for (const c of candidates) {
    const d = Math.hypot(c.screen[0] - cursor[0], c.screen[1] - cursor[1]);
    if (d <= radiusPx && (!best || d < best.distancePx)) best = { value: c.value, distancePx: d };
  }
  return best;
}

/** Unique box corners from a flat position array (vertices repeat per face). */
export function uniqueVertices(positions: ArrayLike<number>, tolerance = 1e-4): Vec3[] {
  const out: Vec3[] = [];
  const seen = new Set<string>();
  for (let i = 0; i + 2 < positions.length; i += 3) {
    const v: Vec3 = [positions[i]!, positions[i + 1]!, positions[i + 2]!];
    const key = v.map((c) => Math.round(c / tolerance)).join(",");
    if (!seen.has(key)) {
      seen.add(key);
      out.push(v);
    }
  }
  return out;
}

export type Measurement = { distance: number; delta: Vec3 };

export function measure(a: Vec3, b: Vec3): Measurement {
  const delta: Vec3 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
  return { distance: Math.hypot(...delta), delta };
}

/** Snaps a drag delta to the move grid and drops vertical motion. */
export function snapMove(delta: Vec3, grid = 0.5): Vec3 {
  const s = (v: number) => {
    const snapped = Math.round(v / grid) * grid;
    return Object.is(snapped, -0) ? 0 : snapped;
  };
  return [s(delta[0]), s(delta[1]), 0];
}

/** True when a pointer moved little enough between down and up to count as a click. */
export function isClick(down: Vec2, up: Vec2, slopPx = 4): boolean {
  return Math.hypot(up[0] - down[0], up[1] - down[1]) <= slopPx;
}

export type SnapKind = "close" | "endpoint" | "midpoint" | "horizontal" | "vertical" | "grid" | "free";

/**
 * Sketch snapping, in priority order: closing the loop being drawn, an
 * existing endpoint, a segment midpoint, horizontal/vertical alignment with
 * the previous point, then the grid. `tolerance` is in sketch units (the
 * caller converts its pixel radius).
 */
export function sketchSnap(
  p: Vec2,
  ctx: { vertices: Vec2[]; segments: [Vec2, Vec2][]; drawing: Vec2[]; grid: number; tolerance: number },
): { p: Vec2; kind: SnapKind } {
  const near = (q: Vec2) => Math.hypot(q[0] - p[0], q[1] - p[1]) <= ctx.tolerance;
  const first = ctx.drawing[0];
  if (first && ctx.drawing.length >= 3 && near(first)) return { p: first, kind: "close" };
  let best: { q: Vec2; d: number } | null = null;
  for (const v of [...ctx.vertices, ...ctx.drawing]) {
    const d = Math.hypot(v[0] - p[0], v[1] - p[1]);
    if (d <= ctx.tolerance && (!best || d < best.d)) best = { q: v, d };
  }
  if (best) return { p: best.q, kind: "endpoint" };
  for (const [a, b] of ctx.segments) {
    const m: Vec2 = [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2];
    if (near(m)) return { p: m, kind: "midpoint" };
  }
  const g = ctx.grid > 0 ? ctx.grid : 1;
  const round = (v: number) => Math.round(v / g) * g;
  const last = ctx.drawing[ctx.drawing.length - 1];
  if (last) {
    if (Math.abs(p[1] - last[1]) <= ctx.tolerance) return { p: [round(p[0]), last[1]], kind: "horizontal" };
    if (Math.abs(p[0] - last[0]) <= ctx.tolerance) return { p: [last[0], round(p[1])], kind: "vertical" };
  }
  return { p: [round(p[0]), round(p[1])], kind: "grid" };
}
