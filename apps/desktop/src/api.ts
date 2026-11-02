import type { FeatureDraft, IpcResult, LiquidCadApi, MateInput, RendererRequest, SimParams } from "../shared/ipc";
import type { CutPlan, EngineValue, PreviewResult, RevisionSummary, SelectionInfo, SessionState, Sheet, SimResult } from "./types";

declare global {
  interface Window {
    liquidcad?: LiquidCadApi;
  }
}

export class EngineRequestError extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

export function bridge(): LiquidCadApi {
  if (!window.liquidcad) {
    throw new EngineRequestError("no_bridge", "The native liquidCAD bridge is unavailable. Run the desktop app, not a browser tab.");
  }
  return window.liquidcad;
}

/** Unwraps an IPC result or throws its error. */
export function unwrap<T>(result: IpcResult<T>): T {
  if (!result.ok) throw new EngineRequestError(result.error.code, result.error.message);
  return result.value;
}

export function expect<K extends EngineValue["type"]>(value: unknown, type: K): Extract<EngineValue, { type: K }> {
  const v = value as EngineValue;
  if (!v || typeof v !== "object" || v.type !== type) {
    throw new EngineRequestError("protocol", `expected ${type}, got ${(v as { type?: string })?.type ?? typeof v}`);
  }
  return v as Extract<EngineValue, { type: K }>;
}

/** A session result, or null when the engine superseded the request. */
export function sessionOf(value: unknown): SessionState | null {
  const v = value as EngineValue;
  if (v?.type === "superseded") return null;
  return expect(value, "session").data;
}

async function call(request: RendererRequest): Promise<unknown> {
  return unwrap(await bridge().request(request));
}

export const engine = {
  document: async () => sessionOf(await call({ method: "document" })),
  setParameter: async (name: string, value: number) => sessionOf(await call({ method: "setParameter", params: { name, value } })),
  setMaterial: async (feature: string, material: string) =>
    sessionOf(await call({ method: "setMaterial", params: { feature, material } })),
  setSuppressed: async (feature: string, suppressed: boolean) =>
    sessionOf(await call({ method: "setSuppressed", params: { feature, suppressed } })),
  movePart: async (id: string, offset: [number, number, number]) => sessionOf(await call({ method: "movePart", params: { id, offset } })),
  select: async (id: string | null): Promise<SelectionInfo | null> =>
    expect(await call({ method: "select", params: { id } }), "selection").data,
  undo: async () => sessionOf(await call({ method: "undo" })),
  redo: async () => sessionOf(await call({ method: "redo" })),
  history: async (limit = 50): Promise<RevisionSummary[]> => expect(await call({ method: "history", params: { limit } }), "history").data,
  restore: async (revision: number) => sessionOf(await call({ method: "restore", params: { revision } })),
  drawing: async (): Promise<Sheet> => expect(await call({ method: "drawing" }), "drawing").data,
  cutPlan: async (): Promise<CutPlan> => expect(await call({ method: "cutPlan" }), "cutPlan").data,
  addFeature: async (draft: FeatureDraft) => sessionOf(await call({ method: "addFeature", params: { draft } })),
  editFeature: async (id: string, draft: FeatureDraft) => sessionOf(await call({ method: "editFeature", params: { id, draft } })),
  deleteFeature: async (id: string) => sessionOf(await call({ method: "deleteFeature", params: { id } })),
  previewFeature: async (draft: FeatureDraft): Promise<PreviewResult> =>
    expect(await call({ method: "previewFeature", params: { draft } }), "preview").data,
  addMate: async (mate: MateInput) => sessionOf(await call({ method: "addMate", params: { mate } })),
  deleteMate: async (id: string) => sessionOf(await call({ method: "deleteMate", params: { id } })),
  simulate: async (params: SimParams): Promise<SimResult> =>
    expect(await call({ method: "simulate", params: { params } }), "simulation").data,
};
