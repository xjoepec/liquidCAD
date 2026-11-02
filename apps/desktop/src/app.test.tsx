import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LiquidCadApi } from "../shared/ipc";
import { EngineRequestError, expect as expectValue, sessionOf, unwrap } from "./api";
import { ParameterField } from "./panels/Inspector";
import { applySession, getState, setState } from "./store";
import type { MassProps, Parameter, SessionState } from "./types";

const mass = (m: number): MassProps => ({
  mass_lb: m,
  volume_in3: 1,
  com: [0, 0, 0],
  inertia: [
    [1, 0, 0],
    [0, 1, 0],
    [0, 0, 1],
  ],
  principal: [1, 1, 1],
  axes: [
    [1, 0, 0],
    [0, 1, 0],
    [0, 0, 1],
  ],
});

function session(revision: number, width = 96): SessionState {
  return {
    document: {
      project: {
        schema: 2,
        id: "proj.test",
        name: "Test dock",
        units: "inch",
        parameters: {},
        features: [],
        materials: {},
        placements: {},
        mates: [],
      },
      components: [
        {
          id: "float.r0.c0",
          name: "Float R1C1",
          kind: "float_module",
          feature_id: "feat.floats",
          material: "hdpe_float",
          shape: { type: "box", size: [width / 4, 24, 16] },
          pose: { origin: [0, 0, 0], rotation: [1, 0, 0, 0] },
          bounds: { origin: [0, 0, 0], size: [width / 4, 24, 16] },
          mass: mass(28),
          stock: null,
          placement: [0, 0, 0],
        },
      ],
      sketches: [],
      scene: { meshes: [], bounds_min: [0, 0, 0], bounds_max: [1, 1, 1] },
      bom: [],
      diagnostics: [],
      engineering: {
        mass: mass(28),
        floating: mass(28),
        deck_top_in: 25,
        center_of_buoyancy: [0, 0, 0],
        gm_transverse_in: 10,
        gm_longitudinal_in: 40,
        kb_in: 1,
        kg_in: 5,
        heel_static_deg: 0,
        trim_static_deg: 0,
        heel_crowd_deg: 1,
        edge_freeboard_crowd_in: 10,
        float_count: 1,
        deck_area_ft2: 1,
        dead_load_lb: 1,
        live_load_lb: 1,
        buoyancy_capacity_lb: 10,
        draft_dead_in: 1,
        draft_loaded_in: 2,
        freeboard_dead_in: 20,
        freeboard_loaded_in: 19,
        reserve_buoyancy_ratio: 0.5,
        center_of_gravity: [0, 0, 0],
        deck_span_in: 24,
        stringer_span_in: 48,
      },
      mates: [],
      revision,
    },
    selected: null,
    kernel: "test",
    project_path: null,
    dirty: false,
    can_undo: false,
    can_redo: false,
    undo_label: null,
    redo_label: null,
    rebuild: { regenerated: ["feat.floats"], reused: [], elapsed_ms: 1 },
    meshes_built: 1,
    meshes_reused: 0,
  };
}

const width: Parameter = {
  name: "width_in",
  label: "Dock width",
  value: 96,
  unit: "inch",
  min: 48,
  max: 240,
  step: 24,
  snap: true,
  group: "plan",
};

let request: ReturnType<typeof vi.fn>;

beforeEach(() => {
  request = vi.fn(async (req: { method: string }) => {
    if (req.method === "setParameter") return { ok: true, value: { type: "session", data: session(5, 144) } };
    if (req.method === "history") return { ok: true, value: { type: "history", data: [] } };
    if (req.method === "select") return { ok: true, value: { type: "selection", data: null } };
    return { ok: false, error: { code: "unexpected", message: req.method } };
  });
  window.liquidcad = { request } as unknown as LiquidCadApi;
  setState({ session: null, selectedId: null, selection: null, paramErrors: {}, log: [], pending: 0 });
});

afterEach(() => {
  cleanup();
  delete window.liquidcad;
});

describe("IPC result handling", () => {
  it("treats superseded replies as no-ops and type-checks the rest", () => {
    expect(sessionOf({ type: "superseded" })).toBeNull();
    expect(sessionOf({ type: "session", data: session(1) })?.document.revision).toBe(1);
    expect(() => expectValue({ type: "pong", data: {} }, "session")).toThrow(/expected session/);
    expect(() => unwrap({ ok: false, error: { code: "validation", message: "bad" } })).toThrow(EngineRequestError);
  });
});

describe("store", () => {
  it("never rolls the screen back to an older revision", () => {
    applySession(session(4));
    applySession(session(3));
    expect(getState().session?.document.revision).toBe(4);
    applySession(session(6));
    expect(getState().session?.document.revision).toBe(6);
  });
});

describe("parameter field", () => {
  it("catches invalid input locally without calling the engine", () => {
    render(<ParameterField parameter={width} />);
    const input = screen.getByTestId("param-width_in");
    fireEvent.change(input, { target: { value: "100" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(screen.getByTestId("param-error-width_in").textContent).toContain("96 or 120");
    expect(request).not.toHaveBeenCalled();
  });

  it("sends valid edits to the engine and applies the new session", async () => {
    render(<ParameterField parameter={width} />);
    const input = screen.getByTestId("param-width_in");
    fireEvent.change(input, { target: { value: "144" } });
    await act(async () => {
      fireEvent.keyDown(input, { key: "Enter" });
    });
    expect(request).toHaveBeenCalledWith({ method: "setParameter", params: { name: "width_in", value: 144 } });
    expect(getState().session?.document.revision).toBe(5);
    expect(getState().session?.document.components[0]?.bounds.size[0]).toBe(36);
  });
});
