import { describe, expect, it } from "vitest";
import { ExportFormat, MenuCommand, RendererRequest } from "./ipc";

describe("renderer request allow-list", () => {
  it("accepts every engine request the UI sends", () => {
    const ok = [
      { method: "ping" },
      { method: "document" },
      { method: "setParameter", params: { name: "width_in", value: 144 } },
      { method: "setMaterial", params: { feature: "feat.deck", material: "composite_deck" } },
      { method: "setSuppressed", params: { feature: "feat.cleats", suppressed: true } },
      { method: "movePart", params: { id: "cleat.port.mid", offset: [0, 12.5, 0] } },
      { method: "select", params: { id: "float.r0.c0" } },
      { method: "select", params: { id: null } },
      { method: "undo" },
      { method: "redo" },
      { method: "history", params: { limit: 40 } },
      { method: "restore", params: { revision: 3 } },
      { method: "drawing" },
      { method: "cutPlan" },
    ];
    for (const request of ok) {
      expect(RendererRequest.safeParse(request).success, JSON.stringify(request)).toBe(true);
    }
  });

  it("refuses anything that names a file", () => {
    for (const request of [
      { method: "open", params: { path: "/etc/passwd" } },
      { method: "save", params: { path: "/tmp/x.json" } },
      { method: "export", params: { format: "step", path: "/tmp/x.step" } },
    ]) {
      expect(RendererRequest.safeParse(request).success).toBe(false);
    }
  });

  it("rejects smuggled keys, non-finite numbers, and odd identifiers", () => {
    expect(RendererRequest.safeParse({ method: "undo", params: { path: "/x" } }).success).toBe(false);
    expect(
      RendererRequest.safeParse({ method: "setParameter", params: { name: "width_in", value: 1, path: "/x" } }).success,
    ).toBe(false);
    expect(RendererRequest.safeParse({ method: "setParameter", params: { name: "width_in", value: Infinity } }).success).toBe(false);
    expect(RendererRequest.safeParse({ method: "setParameter", params: { name: "width_in", value: Number.NaN } }).success).toBe(false);
    expect(RendererRequest.safeParse({ method: "select", params: { id: "../../secret" } }).success).toBe(false);
    expect(RendererRequest.safeParse({ method: "restore", params: { revision: -1 } }).success).toBe(false);
    expect(RendererRequest.safeParse({ method: "movePart", params: { id: "cleat.port.mid", offset: [1, 2] } }).success).toBe(false);
  });

  it("knows exactly four export formats and the menu commands", () => {
    expect(ExportFormat.options).toEqual(["dxf", "step", "pdf", "csv"]);
    expect(ExportFormat.safeParse("stl").success).toBe(false);
    expect(MenuCommand.safeParse("undo").success).toBe(true);
    expect(MenuCommand.safeParse("rm -rf").success).toBe(false);
  });
});

describe("feature, mate, and simulation requests", () => {
  const bracket = {
    name: "Bracket",
    spec: {
      type: "extrude",
      plane: { on: "face", component: "deck.board.6", face: "z+" },
      profile: { outer: [{ x: 0, y: 0 }, { x: 6, y: 0 }, { x: 6, y: 2 }, { x: 0, y: 2 }] },
      depth: 0.5,
      direction: "forward",
    },
  };

  it("accepts a sketched part and a fillet", () => {
    expect(RendererRequest.safeParse({ method: "addFeature", params: { draft: bracket } }).success).toBe(true);
    const fillet = {
      name: "Ease edge",
      spec: { type: "modify", target: "deck.board.0", op: { op: "fillet", edges: ["y-|z+", "fillet(side.0|top)#2"], radius: 0.25 } },
    };
    expect(RendererRequest.safeParse({ method: "previewFeature", params: { draft: fillet } }).success).toBe(true);
  });

  it("rejects paths, bad names, huge profiles, and arcs past 180 degrees", () => {
    const bad = (draft: unknown) => RendererRequest.safeParse({ method: "addFeature", params: { draft } }).success;
    expect(bad({ ...bracket, spec: { ...bracket.spec, plane: { on: "face", component: "../etc", face: "z+" } } })).toBe(false);
    expect(bad({ ...bracket, spec: { ...bracket.spec, plane: { on: "face", component: "deck.board.6", face: "z+;rm" } } })).toBe(false);
    const huge = Array.from({ length: 501 }, (_, i) => ({ x: i, y: i % 2 }));
    expect(bad({ ...bracket, spec: { ...bracket.spec, profile: { outer: huge } } })).toBe(false);
    expect(bad({ ...bracket, spec: { ...bracket.spec, profile: { outer: [{ x: 0, y: 0, bulge: 2 }, { x: 1, y: 0 }] } } })).toBe(false);
    expect(bad({ ...bracket, spec: { ...bracket.spec, depth: -1 } })).toBe(false);
    expect(bad({ ...bracket, path: "/tmp/x" })).toBe(false);
  });

  it("bounds simulation inputs", () => {
    const ok = { wave_height_in: 12, period_s: 2.5, heading_deg: 90, duration_s: 20 };
    expect(RendererRequest.safeParse({ method: "simulate", params: { params: ok } }).success).toBe(true);
    expect(RendererRequest.safeParse({ method: "simulate", params: { params: { ...ok, duration_s: 600 } } }).success).toBe(false);
    expect(RendererRequest.safeParse({ method: "simulate", params: { params: { ...ok, period_s: Number.NaN } } }).success).toBe(false);
  });

  it("checks mates", () => {
    const mate = {
      id: "mate.user.1",
      name: "Hinge",
      kind: "revolute",
      parent: { component: "deck.board.0", edge: "y-|z+" },
      child: { component: "gangway.hinge", edge: "y-|z-" },
      parameter: "gangway_angle_deg",
      value: 0,
      invert: true,
      limits: [-20, 30],
    };
    expect(RendererRequest.safeParse({ method: "addMate", params: { mate } }).success).toBe(true);
    expect(RendererRequest.safeParse({ method: "addMate", params: { mate: { ...mate, kind: "weld" } } }).success).toBe(false);
  });
});
