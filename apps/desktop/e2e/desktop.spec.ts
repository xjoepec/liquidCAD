// End-to-end: the first working milestone, driven through the real Electron
// app, the real preload bridge, and the real Rust engine.
import { _electron as electron, expect, test, type ElectronApplication, type Page } from "@playwright/test";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const desktop = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const root = path.resolve(desktop, "../..");
const exe = process.platform === "win32" ? "liquidcad.exe" : "liquidcad";
const screens = path.join(root, "artifacts", "screens");
// LIQUIDCAD_RECORD=1 (npm run demo) records the whole run to artifacts/videos,
// with each test's start time, so scripts/encode-demos.mjs can cut one clip per test.
const record = Boolean(process.env.LIQUIDCAD_RECORD);
const videos = path.join(root, "artifacts", "videos");
const marks: { title: string; start: number; end?: number }[] = [];
let videoStart = 0;

// Steps build on each other, like a user session: stop at the first failure.
test.describe.configure({ mode: "serial" });

let app: ElectronApplication;
let page: Page;
let exportDir: string;

test.beforeAll(async () => {
  const engine = process.env.LIQUIDCAD_ENGINE ?? path.join(root, "target", "debug", exe);
  if (!fs.existsSync(engine)) throw new Error(`build the engine first: cargo build -p liquidcad-cli (${engine})`);
  if (!fs.existsSync(path.join(desktop, "dist", "index.html"))) throw new Error("run npm run build first");
  const userData = fs.mkdtempSync(path.join(os.tmpdir(), "liquidcad-e2e-profile-"));
  exportDir = fs.mkdtempSync(path.join(os.tmpdir(), "liquidcad-e2e-out-"));
  fs.mkdirSync(screens, { recursive: true });
  const env = { ...process.env, LIQUIDCAD_ENGINE: engine, LIQUIDCAD_USER_DATA: userData } as Record<string, string>;
  delete env.LIQUIDCAD_DEV_URL;
  delete env.ELECTRON_RUN_AS_NODE;
  app = await electron.launch({
    args: [desktop],
    cwd: desktop,
    env,
    ...(record ? { recordVideo: { dir: path.join(videos, "raw"), size: { width: 1600, height: 1000 } } } : {}),
  });
  page = await app.firstWindow();
  videoStart = Date.now();
  page.on("pageerror", (e) => console.log(`[pageerror] ${e.message}`));
  page.on("crash", () => console.log("[renderer crashed]"));
  page.on("framenavigated", (f) => console.log(`[navigated] ${f.url()}`));
  page.on("console", (m) => m.type() === "error" && console.log(`[console.error] ${m.text()}`));
  await page.setViewportSize({ width: 1600, height: 1000 }).catch(() => undefined);
  // Native save dialogs cannot be clicked by automation; answer them with a path in exportDir.
  await app.evaluate(({ dialog }, dir) => {
    dialog.showSaveDialog = (async (...args: unknown[]) => {
      const options = (args.length > 1 ? args[1] : args[0]) as { defaultPath?: string };
      return { canceled: false, filePath: `${dir}/${options.defaultPath ?? "out.bin"}` };
    }) as typeof dialog.showSaveDialog;
  }, exportDir);
});

test.afterAll(async () => {
  if (!app) return;
  if (record) test.setTimeout(660_000);
  if (record) fs.writeFileSync(path.join(videos, "e2e.json"), JSON.stringify(marks, null, 2));
  // Close politely, but never let a stuck window hold the run hostage. When
  // recording, closing waits for Playwright's encoder to drain its backlog,
  // which can take minutes; killing it early truncates the video.
  const closed = await Promise.race([
    app.close().then(() => true),
    new Promise<boolean>((resolve) => setTimeout(() => resolve(false), record ? 600_000 : 15_000)),
  ]);
  if (!closed) app.process().kill("SIGKILL");
  if (record && closed) await page.video()?.saveAs(path.join(videos, "e2e.webm"));
});

test.beforeEach(({}, info) => {
  if (record) marks.push({ title: info.title, start: (Date.now() - videoStart) / 1000 });
});

test.afterEach(() => {
  if (record) marks[marks.length - 1].end = (Date.now() - videoStart) / 1000;
});

/** Clicks a part row, expanding its tree group only if the row is hidden. */
async function pickInTree(id: string, group: RegExp) {
  const row = page.locator(`[data-id="${id}"]`);
  if (!(await row.isVisible())) await page.getByRole("button", { name: group }).click();
  await row.click();
}

const meshCount = () => page.getByTestId("viewport").getAttribute("data-mesh-count");

test("1-4: desktop window starts the engine and shows the generated dock", async () => {
  await expect(page.getByTestId("engine-state")).toHaveAttribute("data-state", "ready");
  await expect(page.getByTestId("engine-state")).toContainText(/B-rep|OpenCascade/);
  await page.getByTestId("open-sample").click();
  // 84 dock parts plus the 27-part hinged gangway.
  await expect(page.getByTestId("viewport")).toHaveAttribute("data-mesh-count", "111");
  await expect(page.getByTestId("assembly-tree")).toContainText("Float pattern");
  await page.waitForTimeout(400);
  await page.screenshot({ path: path.join(screens, "01-model-open.png") });
});

test("5: picking in the viewport and the assembly tree selects the same model", async () => {
  const point = await page.evaluate(() => window.__liquidcadViewport?.screenPoint("deck.board.16") ?? null);
  expect(point).not.toBeNull();
  await page.mouse.click(point!.x, point!.y);
  await expect(page.getByTestId("selection")).toContainText(/deck\.board\.\d+/);
  await expect(page.getByTestId("selection")).toContainText("Deck boards");

  await page.locator('[data-id="frame.stringer.stbd"]').click();
  await expect(page.getByTestId("selection")).toContainText("frame.stringer.stbd");
  await expect(page.getByTestId("selection")).toContainText("94, 0, 20");
});

test("6-9: editing width regenerates geometry and the BOM without a restart", async () => {
  const before = Number(await page.getByTestId("viewport").getAttribute("data-revision"));
  const width = page.getByTestId("param-width_in");
  await width.fill("100");
  await width.press("Enter");
  await expect(page.getByTestId("param-error-width_in")).toContainText("96 or 120");

  await width.fill("144");
  await width.press("Enter");
  await expect(page.getByTestId("viewport")).toHaveAttribute("data-mesh-count", "129");
  expect(Number(await page.getByTestId("viewport").getAttribute("data-revision"))).toBeGreaterThan(before);
  // Same semantic id, new geometry.
  await expect(page.getByTestId("selection")).toContainText("frame.stringer.stbd");
  await expect(page.getByTestId("selection")).toContainText("142, 0, 20");
  await expect(page.getByTestId("statusbar")).toContainText(/regenerated \d+, reused \d+ features/);

  await page.getByTestId("tab-bom").click();
  await expect(page.getByTestId("bom-summary-qty-float_module")).toHaveText("48");
  await expect(page.getByTestId("bom-summary-qty-stringer")).toHaveText("7");
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(screens, "02-model-width-144.png") });
});

test("materials drive diagnostics; undo and redo restore them", async () => {
  await pickInTree("deck.board.0", /Deck boards/);
  await page.getByTestId("material-select").selectOption("composite_deck");
  await page.getByTestId("tab-diagnostics").click();
  await expect(page.getByTestId("diagnostics")).toContainText("SPAN-01");
  await page.getByTestId("undo").click();
  await expect(page.getByTestId("diagnostics")).not.toContainText("SPAN-01");
  await page.getByTestId("redo").click();
  await expect(page.getByTestId("diagnostics")).toContainText("SPAN-01");
  await page.getByTestId("undo").click();
  await expect(page.getByTestId("diagnostics")).not.toContainText("SPAN-01");
  await page.getByTestId("tab-history").click();
  await expect(page.getByTestId("history")).toContainText("Dock width = 144");
});

test("cleats move through the engine with snapping and stable ids", async () => {
  await pickInTree("cleat.port.mid", /Mooring cleats/);
  await page.getByTestId("place-dy").fill("12.3");
  await page.getByTestId("place-dy").press("Enter");
  await expect(page.getByTestId("place-dy")).toHaveValue("12.5");
  await page.getByTestId("place-dx").fill("-40");
  await page.getByTestId("place-dx").press("Enter");
  await page.getByTestId("tab-output").click();
  await expect(page.getByTestId("output-log")).toContainText("must stay on the");
});

test("10: exports real DXF, STEP, PDF and CSV for the edited dock", async () => {
  for (const format of ["dxf", "step", "pdf", "csv"]) {
    await page.getByTestId("export-menu").click();
    await page.getByTestId(`export-${format}`).click();
    await expect(page.getByTestId("output-log")).toContainText(`exported ${format.toUpperCase()}`);
  }
  const file = (ext: string) => path.join(exportDir, `modular-floating-dock.${ext}`);
  const dxf = fs.readFileSync(file("dxf"), "utf8");
  expect(dxf).toContain("AC1009");
  expect(dxf).toContain("144 IN");
  const step = fs.readFileSync(file("step"), "utf8");
  expect(step.startsWith("ISO-10303-21;")).toBe(true);
  expect(step.match(/MANIFOLD_SOLID_BREP\(/g)?.length).toBe(129);
  expect(step).toContain("'cleat.port.mid'");
  const pdf = fs.readFileSync(file("pdf"));
  expect(pdf.subarray(0, 8).toString()).toBe("%PDF-1.4");
  expect(pdf.subarray(-6).toString()).toBe("%%EOF\n");
  const csv = fs.readFileSync(file("csv"), "utf8");
  expect(csv).toMatch(/,float_module,.*,48,/);
  fs.mkdirSync(path.join(root, "artifacts", "e2e"), { recursive: true });
  for (const ext of ["dxf", "step", "pdf", "csv"]) fs.copyFileSync(file(ext), path.join(root, "artifacts", "e2e", `dock.${ext}`));
});

test("sketch, drawing and fabrication workspaces follow the model", async () => {
  await page.getByTestId("workspace-sketch").click();
  await expect(page.getByTestId("sketch")).toBeVisible();
  const handle = page.getByTestId("handle-length");
  const box = (await handle.boundingBox())!;
  const lengthBefore = Number(await page.getByTestId("param-length_in").first().inputValue());
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 + 90, box.y + box.height / 2, { steps: 8 });
  await page.mouse.up();
  await expect
    .poll(async () => Number(await page.getByTestId("param-length_in").first().inputValue()))
    .toBeGreaterThan(lengthBefore);
  const lengthAfter = Number(await page.getByTestId("param-length_in").first().inputValue());
  expect(lengthAfter % 24).toBe(0);
  await page.screenshot({ path: path.join(screens, "03-sketch.png") });

  await page.getByTestId("workspace-drawing").click();
  await expect(page.getByTestId("drawing-sheet")).toContainText("FRAMING PLAN");
  await expect(page.getByTestId("drawing-sheet")).toContainText("144 in");
  await page.screenshot({ path: path.join(screens, "04-drawing.png") });

  await page.getByTestId("workspace-fab").click();
  await expect(page.getByTestId("bom-qty-float_module")).toHaveText(String((144 / 24) * (lengthAfter / 24)));
  await expect(page.locator(".stock-group").first()).toBeVisible();
  await page.screenshot({ path: path.join(screens, "05-fabrication.png") });

  await page.getByTestId("workspace-model").click();
  await expect.poll(meshCount).not.toBe("0");
});

const topoPoint = (component: string, kind: "face" | "edge", id: string) =>
  page.evaluate(([c, k, i]) => window.__liquidcadViewport?.topoPoint(c, k as "face" | "edge", i) ?? null, [component, kind, id] as const);

const native = async () => /OpenCascade \d/.test((await page.getByTestId("engine-state").textContent()) ?? "");

test("faces and edges are picked by construction name and measured", async () => {
  await page.getByRole("button", { name: "Iso", exact: true }).click();
  await page.waitForTimeout(300);
  await page.getByTestId("pick-mode").getByRole("button", { name: "Face" }).click();
  const top = await topoPoint("deck.board.6", "face", "z+");
  expect(top).not.toBeNull();
  await page.mouse.click(top!.x, top!.y);
  await expect(page.getByTestId("picks")).toContainText("z+");
  await expect(page.getByTestId("picks")).toContainText(/plane, 792(\.0+)? in²/);
  // A front-row float's stern face faces the iso camera unobstructed.
  const front = await topoPoint("float.r0.c0", "face", "y-");
  await page.keyboard.down("Shift");
  await page.mouse.click(front!.x, front!.y);
  await page.keyboard.up("Shift");
  await expect(page.getByTestId("pick-relation")).toContainText("90 deg between normals");
  await page.getByTestId("toggle-mass").click();
  await expect(page.getByTestId("stability")).toContainText("GM across");
  await expect(page.getByTestId("selection")).toContainText("Mass properties");
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(screens, "06-face-picks-and-mass.png") });
  await page.getByTestId("toggle-mass").click();
});

test("a part sketched on a deck board is extruded, scheduled, and drawn", async () => {
  await page.keyboard.press("Escape");
  const top = await topoPoint("deck.board.6", "face", "z+");
  await page.mouse.click(top!.x, top!.y);
  await page.getByTestId("sketch-on-face").click();
  await expect(page.getByTestId("part-sketch")).toBeVisible();
  const canvas = (await page.getByTestId("sketch-canvas").boundingBox())!;
  const cx = canvas.x + canvas.width / 2;
  const cy = canvas.y + canvas.height / 2;
  await page.getByTestId("sk-tool-rect").click();
  await page.mouse.click(cx - 60, cy - 30);
  await page.mouse.click(cx + 60, cy + 30);
  await page.getByTestId("sk-name").fill("Ladder bracket");
  await page.getByTestId("sk-depth").fill("0.5");
  await expect(page.getByTestId("sk-preview")).toContainText(" lb");
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(screens, "07-part-sketch.png") });
  await page.getByTestId("sk-create").click();
  await expect(page.getByTestId("assembly-tree")).toContainText("Ladder bracket");
  await page.getByTestId("tab-bom").click();
  await expect(page.getByTestId("bom-summary")).toContainText("Ladder bracket");
  await page.getByTestId("tab-output").click();
  await page.getByTestId("export-menu").click();
  await page.getByTestId("export-dxf").click();
  await expect(page.getByTestId("output-log").locator("li").first()).toContainText("exported DXF");
  const dxf = fs.readFileSync(path.join(exportDir, "modular-floating-dock.dxf"), "utf8");
  expect(dxf).toContain("LADDER BRACKET x1  T 0.5 IN");
  expect(dxf).toContain("\nCUSTOM\n");
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(screens, "08-bracket-on-deck.png") });
});

test("edges take fillets through OpenCascade, or say why they cannot", async () => {
  await page.getByTestId("pick-mode").getByRole("button", { name: "Edge" }).click();
  const edge = await topoPoint("deck.board.10", "edge", "y-|z+");
  expect(edge).not.toBeNull();
  await page.mouse.click(edge!.x, edge!.y);
  await expect(page.getByTestId("picks")).toContainText("y-|z+");
  if (await native()) {
    const before = await page.getByTestId("part-mass").textContent();
    await page.getByTestId("edge-size").fill("0.25");
    await page.getByTestId("add-fillet").click();
    await expect(page.getByTestId("selection")).toContainText("1 modifier(s)");
    await expect(page.getByTestId("part-mass")).not.toHaveText(before ?? "");
    await page.getByTestId("pick-mode").getByRole("button", { name: "Face" }).click();
    const round = await topoPoint("deck.board.10", "face", "fillet(y-|z+)");
    expect(round).not.toBeNull();
    await page.mouse.click(round!.x, round!.y);
    await expect(page.getByTestId("picks")).toContainText("cylinder");
  } else {
    await expect(page.getByTestId("add-fillet")).toBeDisabled();
    await expect(page.getByTestId("picks")).toContainText("Needs OpenCascade");
  }
  await page.waitForTimeout(300);
  await page.screenshot({ path: path.join(screens, "09-edge-fillet.png") });
  await page.getByTestId("pick-mode").getByRole("button", { name: "Part" }).click();
});

test("the gangway slope swings only the gangway about its hinge", async () => {
  await pickInTree("gangway.stringer.port", /Access gangway/);
  const extentsBefore = await page.getByTestId("selection").textContent();
  const angle = page.getByTestId("param-gangway_angle_deg");
  await angle.fill("12");
  await angle.press("Enter");
  await expect(page.getByTestId("statusbar")).toContainText("meshes built 27");
  await page.getByTestId("tab-diagnostics").click();
  await expect(page.getByTestId("diagnostics")).toContainText("GANG-01");
  await expect(page.getByTestId("mates")).toContainText("12.0 deg");
  // The hinge load sits on the hinge line, so the dock's trim does not
  // change; the gangway itself swings up.
  await expect(page.getByTestId("selection")).not.toHaveText(extentsBefore ?? "");
  await page.getByRole("button", { name: "Right", exact: true }).click();
  await page.waitForTimeout(400);
  await page.screenshot({ path: path.join(screens, "10-gangway-12deg.png") });
});

test("wave simulation runs in Rust and plays back", async () => {
  await page.getByTestId("workspace-simulate").click();
  await page.getByTestId("sim-duration").fill("8");
  await page.getByTestId("sim-run").click();
  await expect(page.getByTestId("sim-summary")).toContainText("Heave period");
  await page.getByTestId("tab-output").click();
  await expect(page.getByTestId("output-log")).toContainText("simulated 8 s");
  await page.getByTestId("sim-play").click();
  await page.getByTestId("sim-scrub").fill("5");
  await page.waitForTimeout(400);
  await page.screenshot({ path: path.join(screens, "11-simulate.png") });
  await page.getByTestId("workspace-model").click();
});
