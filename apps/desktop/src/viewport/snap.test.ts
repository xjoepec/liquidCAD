import { describe, expect, it } from "vitest";
import { isClick, measure, nearestScreenPoint, sketchSnap, snapMove, uniqueVertices } from "./snap";

describe("viewport interaction math", () => {
  it("snaps to the closest candidate inside the radius only", () => {
    const candidates = [
      { screen: [100, 100] as [number, number], value: "a" },
      { screen: [108, 100] as [number, number], value: "b" },
      { screen: [300, 300] as [number, number], value: "c" },
    ];
    expect(nearestScreenPoint([106, 101], candidates, 14)?.value).toBe("b");
    expect(nearestScreenPoint([200, 200], candidates, 14)).toBeNull();
  });

  it("collapses a flat-shaded box to its 8 corners", () => {
    const corners = [
      [0, 0, 0],
      [1, 0, 0],
      [1, 2, 0],
      [0, 2, 0],
      [0, 0, 3],
      [1, 0, 3],
      [1, 2, 3],
      [0, 2, 3],
    ];
    // 6 faces x 4 vertices, each corner repeated three times.
    const faces = [
      [4, 5, 6, 7],
      [0, 3, 2, 1],
      [0, 1, 5, 4],
      [2, 3, 7, 6],
      [1, 2, 6, 5],
      [3, 0, 4, 7],
    ];
    const positions = faces.flatMap((f) => f.flatMap((i) => corners[i]!));
    expect(positions.length).toBe(72);
    expect(uniqueVertices(positions)).toHaveLength(8);
  });

  it("measures distance and components", () => {
    const m = measure([0, 0, 0], [3, 4, 12]);
    expect(m.distance).toBe(13);
    expect(m.delta).toEqual([3, 4, 12]);
  });

  it("snaps moves to half inches on the deck plane", () => {
    expect(snapMove([12.3, -0.2, 5])).toEqual([12.5, 0, 0]);
    expect(snapMove([0.74, 0.76, 0], 0.5)).toEqual([0.5, 1, 0]);
  });

  it("tells clicks from orbit drags", () => {
    expect(isClick([10, 10], [12, 11])).toBe(true);
    expect(isClick([10, 10], [30, 10])).toBe(false);
  });
});

describe("sketchSnap", () => {
  const ctx = {
    vertices: [[0, 0], [6, 0]] as [number, number][],
    segments: [[[0, 0], [6, 0]]] as [[number, number], [number, number]][],
    drawing: [] as [number, number][],
    grid: 0.25,
    tolerance: 0.2,
  };
  it("prefers endpoints, then midpoints, then the grid", () => {
    expect(sketchSnap([5.9, 0.1], ctx)).toEqual({ p: [6, 0], kind: "endpoint" });
    expect(sketchSnap([3.05, -0.1], ctx)).toEqual({ p: [3, 0], kind: "midpoint" });
    expect(sketchSnap([1.13, 2.4], ctx)).toEqual({ p: [1.25, 2.5], kind: "grid" });
  });
  it("closes a loop and aligns with the previous point", () => {
    const drawing: [number, number][] = [[10, 10], [14, 10], [14, 13]];
    expect(sketchSnap([10.1, 9.9], { ...ctx, drawing })).toEqual({ p: [10, 10], kind: "close" });
    expect(sketchSnap([11.3, 13.1], { ...ctx, drawing }).kind).toBe("horizontal");
    expect(sketchSnap([14.1, 16.3], { ...ctx, drawing })).toEqual({ p: [14, 16.25], kind: "vertical" });
  });
});
