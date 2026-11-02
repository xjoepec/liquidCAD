// IPC contract shared by the Electron main process and the renderer.
//
// The renderer may only send the engine requests listed in RendererRequest.
// None of them take a filesystem path: opening, saving, and exporting go
// through main-process dialogs, so the renderer never names a file.
import { z } from "zod";

const identifier = z
  .string()
  .min(1)
  .max(128)
  .regex(/^[A-Za-z0-9_.-]+$/, "identifier contains illegal characters");
const finite = z.number().finite();
/** Face and edge construction names: `y-|z+`, `fillet(side.0|top)`, `side.0#2`. */
const topoName = z
  .string()
  .min(1)
  .max(160)
  .regex(/^[A-Za-z0-9_.()|#+-]+$/, "face or edge name contains illegal characters");
const vec3 = z.tuple([finite, finite, finite]);

const PathVertex = z.object({ x: finite, y: finite, bulge: finite.min(-1).max(1).optional() }).strict();
const Loop = z.array(PathVertex).min(2).max(500);
export const ProfileSchema = z.object({ outer: Loop, holes: z.array(Loop).max(50).optional() }).strict();

const SketchPlaneSchema = z.discriminatedUnion("on", [
  z.object({ on: z.literal("xy"), offset: finite }).strict(),
  z.object({ on: z.literal("xz"), offset: finite }).strict(),
  z.object({ on: z.literal("yz"), offset: finite }).strict(),
  z.object({ on: z.literal("face"), component: identifier, face: topoName }).strict(),
]);

const ShapeOpSchema = z.discriminatedUnion("op", [
  z.object({ op: z.literal("fillet"), edges: z.array(topoName).min(1).max(64), radius: finite.positive() }).strict(),
  z.object({ op: z.literal("chamfer"), edges: z.array(topoName).min(1).max(64), distance: finite.positive() }).strict(),
  z
    .object({ op: z.literal("hole"), face: topoName, at: vec3, diameter: finite.positive(), depth: finite.min(0) })
    .strict(),
]);

export const FeatureDraftSchema = z
  .object({
    name: z.string().trim().min(1).max(80),
    material: identifier.nullable().optional(),
    spec: z.discriminatedUnion("type", [
      z
        .object({
          type: z.literal("extrude"),
          plane: SketchPlaneSchema,
          profile: ProfileSchema,
          depth: finite.positive(),
          depth_parameter: identifier.nullable().optional(),
          direction: z.enum(["forward", "backward", "symmetric"]),
          cut_into: identifier.nullable().optional(),
        })
        .strict(),
      z.object({ type: z.literal("modify"), target: identifier, op: ShapeOpSchema }).strict(),
    ]),
  })
  .strict();
export type FeatureDraft = z.infer<typeof FeatureDraftSchema>;

const MateRefSchema = z.object({ component: identifier, edge: topoName }).strict();
export const MateSchema = z
  .object({
    id: identifier,
    name: z.string().trim().min(1).max(80),
    kind: z.enum(["fixed", "revolute", "prismatic"]),
    parent: MateRefSchema,
    child: MateRefSchema,
    parameter: identifier.nullable().optional(),
    value: finite,
    invert: z.boolean(),
    limits: z.tuple([finite, finite]).nullable().optional(),
  })
  .strict();
export type MateInput = z.infer<typeof MateSchema>;

export const SimParamsSchema = z
  .object({
    wave_height_in: finite.min(0).max(120),
    period_s: finite.min(1).max(20),
    heading_deg: finite.min(-360).max(360),
    duration_s: finite.min(1).max(120),
    walker: z
      .object({
        weight_lb: finite.min(0).max(600),
        from: z.tuple([finite, finite]),
        to: z.tuple([finite, finite]),
        speed_in_s: finite.min(1).max(120),
      })
      .strict()
      .nullable()
      .optional(),
  })
  .strict();
export type SimParams = z.infer<typeof SimParamsSchema>;

export const RendererRequest = z.discriminatedUnion("method", [
  z.object({ method: z.literal("ping") }).strict(),
  z.object({ method: z.literal("document") }).strict(),
  z
    .object({
      method: z.literal("setParameter"),
      params: z.object({ name: identifier, value: finite }).strict(),
    })
    .strict(),
  z
    .object({
      method: z.literal("setMaterial"),
      params: z.object({ feature: identifier, material: identifier }).strict(),
    })
    .strict(),
  z
    .object({
      method: z.literal("setSuppressed"),
      params: z.object({ feature: identifier, suppressed: z.boolean() }).strict(),
    })
    .strict(),
  z
    .object({
      method: z.literal("movePart"),
      params: z.object({ id: identifier, offset: z.tuple([finite, finite, finite]) }).strict(),
    })
    .strict(),
  z
    .object({
      method: z.literal("select"),
      params: z.object({ id: identifier.nullable() }).strict(),
    })
    .strict(),
  z.object({ method: z.literal("undo") }).strict(),
  z.object({ method: z.literal("redo") }).strict(),
  z
    .object({
      method: z.literal("history"),
      params: z.object({ limit: z.number().int().min(1).max(500).nullable() }).strict(),
    })
    .strict(),
  z
    .object({
      method: z.literal("restore"),
      params: z.object({ revision: z.number().int().positive() }).strict(),
    })
    .strict(),
  z.object({ method: z.literal("drawing") }).strict(),
  z.object({ method: z.literal("cutPlan") }).strict(),
  z.object({ method: z.literal("addFeature"), params: z.object({ draft: FeatureDraftSchema }).strict() }).strict(),
  z
    .object({ method: z.literal("editFeature"), params: z.object({ id: identifier, draft: FeatureDraftSchema }).strict() })
    .strict(),
  z.object({ method: z.literal("deleteFeature"), params: z.object({ id: identifier }).strict() }).strict(),
  z.object({ method: z.literal("previewFeature"), params: z.object({ draft: FeatureDraftSchema }).strict() }).strict(),
  z.object({ method: z.literal("addMate"), params: z.object({ mate: MateSchema }).strict() }).strict(),
  z.object({ method: z.literal("deleteMate"), params: z.object({ id: identifier }).strict() }).strict(),
  z.object({ method: z.literal("simulate"), params: z.object({ params: SimParamsSchema }).strict() }).strict(),
]);
export type RendererRequest = z.infer<typeof RendererRequest>;

export const ExportFormat = z.enum(["dxf", "step", "pdf", "csv"]);
export type ExportFormat = z.infer<typeof ExportFormat>;

export const MenuCommand = z.enum([
  "undo",
  "redo",
  "workspace:model",
  "workspace:sketch",
  "workspace:drawing",
  "workspace:fab",
  "workspace:simulate",
  "view:iso",
  "view:top",
  "view:front",
  "view:right",
  "view:fit",
  "view:ortho",
  "tool:measure",
  "tool:move",
]);
export type MenuCommand = z.infer<typeof MenuCommand>;

/** Engine process state pushed from main to the renderer. */
export type EngineStatus =
  | { state: "starting" }
  | { state: "ready"; kernel: string; nativeOcct: boolean; version: string }
  | { state: "restarting"; reason: string }
  | { state: "failed"; reason: string };

/** Envelope every main-process handler resolves with. */
export type IpcResult<T> = { ok: true; value: T } | { ok: false; error: { code: string; message: string } };

export type ExportedFile = { format: ExportFormat; path: string; bytes: number };

/** The API the preload script exposes as window.liquidcad. */
export type LiquidCadApi = {
  request: (request: RendererRequest) => Promise<IpcResult<unknown>>;
  status: () => Promise<EngineStatus>;
  openSample: () => Promise<IpcResult<unknown>>;
  openProject: () => Promise<IpcResult<unknown> | null>;
  saveProject: () => Promise<IpcResult<{ path: string }> | null>;
  saveProjectAs: () => Promise<IpcResult<{ path: string }> | null>;
  exportFile: (format: ExportFormat) => Promise<IpcResult<ExportedFile> | null>;
  exportAll: () => Promise<IpcResult<ExportedFile[]> | null>;
  reveal: (path: string) => Promise<void>;
  onMenu: (handler: (command: MenuCommand) => void) => () => void;
  onStatus: (handler: (status: EngineStatus) => void) => () => void;
  /** Sessions produced by native menu commands (File > Open, ...). */
  onPushed: (handler: (result: IpcResult<unknown>) => void) => () => void;
};
