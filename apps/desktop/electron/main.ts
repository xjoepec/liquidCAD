// Electron main process: window lifecycle, native menus and dialogs, and the
// only bridge between the renderer and the Rust engine.
import { app, BrowserWindow, dialog, ipcMain, Menu, shell, type MenuItemConstructorOptions } from "electron";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { PALETTE } from "../shared/color.js";
import { ExportFormat, MenuCommand, RendererRequest, type ExportedFile, type IpcResult } from "../shared/ipc.js";
import { EngineError, EngineSupervisor, engineBinary, sampleProjectPath } from "./engine.js";

const here = path.dirname(fileURLToPath(import.meta.url));
const devUrl = process.env.LIQUIDCAD_DEV_URL;
let window: BrowserWindow | null = null;
let engine: EngineSupervisor | null = null;

/** Session facts main needs to route file commands correctly. */
const session = {
  path: null as string | null,
  isSample: false,
  name: "dock",
  exported: new Set<string>(),
};

// Isolated profiles (tests, portable installs) keep their own history database.
if (process.env.LIQUIDCAD_USER_DATA) {
  app.setPath("userData", process.env.LIQUIDCAD_USER_DATA);
}

if (!app.requestSingleInstanceLock()) {
  app.quit();
}

function ok<T>(value: T): IpcResult<T> {
  return { ok: true, value };
}

function fail(err: unknown): IpcResult<never> {
  if (err instanceof EngineError) return { ok: false, error: { code: err.code, message: err.message } };
  return { ok: false, error: { code: "internal", message: err instanceof Error ? err.message : String(err) } };
}

function slug(name: string): string {
  return name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "project";
}

function remember(result: { type: string; data?: unknown }): void {
  if (result.type !== "session") return;
  const data = result.data as { project_path: string | null; document: { project: { name: string } } };
  session.path = data.project_path;
  session.name = data.document.project.name;
}

async function openPath(file: string, isSample: boolean) {
  const result = await engine!.call({ method: "open", params: { path: file } });
  session.isSample = isSample;
  remember(result);
  updateTitle();
  return result;
}

function updateTitle(): void {
  if (!window) return;
  const where = session.isSample ? "sample (read-only)" : session.path ? path.basename(session.path) : "untitled";
  window.setTitle(`liquidCAD - ${session.name} - ${where}`);
}

function createWindow(): void {
  window = new BrowserWindow({
    width: 1600,
    height: 1000,
    minWidth: 1180,
    minHeight: 720,
    backgroundColor: PALETTE.void,
    title: "liquidCAD",
    show: false,
    webPreferences: {
      preload: path.join(here, "preload.cjs"),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      webSecurity: true,
      spellcheck: false,
    },
  });
  window.once("ready-to-show", () => window?.show());
  window.webContents.setWindowOpenHandler(() => ({ action: "deny" }));
  window.webContents.on("will-navigate", (event, url) => {
    if (!devUrl || !url.startsWith(devUrl)) event.preventDefault();
  });
  window.on("closed", () => {
    window = null;
  });
  if (devUrl) {
    void window.loadURL(devUrl);
  } else {
    void window.loadFile(path.join(here, "../../dist/index.html"));
  }
}

function send(command: MenuCommand): void {
  window?.webContents.send("menu:command", command);
}

function buildMenu(): void {
  const mac = process.platform === "darwin";
  const exportItem = (format: ExportFormat, label: string): MenuItemConstructorOptions => ({
    label,
    click: () => void exportFile(format),
  });
  const template: MenuItemConstructorOptions[] = [
    ...(mac ? [{ role: "appMenu" as const }] : []),
    {
      label: "File",
      submenu: [
        { label: "Open Sample Dock", accelerator: "CmdOrCtrl+Shift+O", click: () => void runAndPush(() => openPath(sampleProjectPath(process.resourcesPath), true)) },
        { label: "Open...", accelerator: "CmdOrCtrl+O", click: () => void runAndPush(openDialog) },
        { type: "separator" },
        { label: "Save", accelerator: "CmdOrCtrl+S", click: () => void saveProject() },
        { label: "Save As...", accelerator: "CmdOrCtrl+Shift+S", click: () => void saveProjectAs() },
        { type: "separator" },
        {
          label: "Export",
          submenu: [
            exportItem("dxf", "DXF (fabrication plan)..."),
            exportItem("step", "STEP AP214 (solids)..."),
            exportItem("pdf", "PDF (drawing sheet)..."),
            exportItem("csv", "CSV (bill of materials)..."),
            { type: "separator" },
            { label: "All Formats to Folder...", accelerator: "CmdOrCtrl+E", click: () => void exportAll() },
          ],
        },
        { type: "separator" },
        mac ? { role: "close" } : { role: "quit" },
      ],
    },
    {
      label: "Edit",
      submenu: [
        { label: "Undo", accelerator: "CmdOrCtrl+Z", click: () => send("undo") },
        { label: "Redo", accelerator: mac ? "Cmd+Shift+Z" : "Ctrl+Y", click: () => send("redo") },
        { type: "separator" },
        { role: "cut" },
        { role: "copy" },
        { role: "paste" },
        { role: "selectAll" },
      ],
    },
    {
      label: "View",
      submenu: [
        { label: "Model", accelerator: "CmdOrCtrl+1", click: () => send("workspace:model") },
        { label: "Sketch", accelerator: "CmdOrCtrl+2", click: () => send("workspace:sketch") },
        { label: "Drawing", accelerator: "CmdOrCtrl+3", click: () => send("workspace:drawing") },
        { label: "Fabrication", accelerator: "CmdOrCtrl+4", click: () => send("workspace:fab") },
        { label: "Simulate", accelerator: "CmdOrCtrl+5", click: () => send("workspace:simulate") },
        { type: "separator" },
        { label: "Isometric", click: () => send("view:iso") },
        { label: "Top", click: () => send("view:top") },
        { label: "Front", click: () => send("view:front") },
        { label: "Right", click: () => send("view:right") },
        { label: "Fit All", click: () => send("view:fit") },
        { label: "Toggle Orthographic", click: () => send("view:ortho") },
        { type: "separator" },
        { label: "Measure", click: () => send("tool:measure") },
        { label: "Move Part", click: () => send("tool:move") },
        { type: "separator" },
        { role: "toggleDevTools" },
        { role: "togglefullscreen" },
      ],
    },
  ];
  Menu.setApplicationMenu(Menu.buildFromTemplate(template));
}

/** Runs a menu action and pushes the resulting session to the renderer. */
async function runAndPush(action: () => Promise<{ type: string; data?: unknown } | null>): Promise<void> {
  try {
    const result = await action();
    if (result) window?.webContents.send("engine:pushed", ok(result));
  } catch (err) {
    window?.webContents.send("engine:pushed", fail(err));
  }
}

async function openDialog() {
  const result = await dialog.showOpenDialog(window!, {
    title: "Open liquidCAD project",
    filters: [{ name: "liquidCAD project", extensions: ["json"] }],
    properties: ["openFile"],
  });
  const file = result.filePaths[0];
  if (result.canceled || !file) return null;
  return openPath(file, false);
}

async function saveProjectAs(): Promise<IpcResult<{ path: string }> | null> {
  const result = await dialog.showSaveDialog(window!, {
    title: "Save project as",
    defaultPath: `${slug(session.name)}.liquidcad.json`,
    filters: [{ name: "liquidCAD project", extensions: ["json"] }],
  });
  if (result.canceled || !result.filePath) return null;
  try {
    const saved = await engine!.call({ method: "save", params: { path: result.filePath } });
    const savedPath = (saved.data as { path: string }).path;
    session.path = savedPath;
    session.isSample = false;
    updateTitle();
    return ok({ path: savedPath });
  } catch (err) {
    return fail(err);
  }
}

async function saveProject(): Promise<IpcResult<{ path: string }> | null> {
  // The bundled sample is a template; never write over it.
  if (session.isSample || !session.path) return saveProjectAs();
  try {
    const saved = await engine!.call({ method: "save", params: { path: null } });
    return ok({ path: (saved.data as { path: string }).path });
  } catch (err) {
    return fail(err);
  }
}

async function exportFile(format: ExportFormat): Promise<IpcResult<ExportedFile> | null> {
  const ext = format;
  const result = await dialog.showSaveDialog(window!, {
    title: `Export ${format.toUpperCase()}`,
    defaultPath: `${slug(session.name)}.${ext}`,
    filters: [{ name: format.toUpperCase(), extensions: format === "step" ? ["step", "stp"] : [ext] }],
  });
  if (result.canceled || !result.filePath) return null;
  try {
    const out = await engine!.call({ method: "export", params: { format, path: result.filePath } });
    const file = out.data as ExportedFile;
    session.exported.add(file.path);
    return ok(file);
  } catch (err) {
    return fail(err);
  }
}

async function exportAll(): Promise<IpcResult<ExportedFile[]> | null> {
  const result = await dialog.showOpenDialog(window!, {
    title: "Export DXF, STEP, PDF and CSV to folder",
    properties: ["openDirectory", "createDirectory"],
  });
  const dir = result.filePaths[0];
  if (result.canceled || !dir) return null;
  try {
    const files: ExportedFile[] = [];
    for (const format of ExportFormat.options) {
      const target = path.join(dir, `${slug(session.name)}.${format}`);
      const out = await engine!.call({ method: "export", params: { format, path: target } });
      const file = out.data as ExportedFile;
      session.exported.add(file.path);
      files.push(file);
    }
    return ok(files);
  } catch (err) {
    return fail(err);
  }
}

function registerIpc(): void {
  ipcMain.handle("engine:request", async (_event, raw: unknown) => {
    const parsed = RendererRequest.safeParse(raw);
    if (!parsed.success) {
      return { ok: false, error: { code: "rejected", message: `invalid request: ${parsed.error.issues[0]?.message ?? "schema"}` } };
    }
    try {
      return ok(await engine!.call(parsed.data));
    } catch (err) {
      return fail(err);
    }
  });
  ipcMain.handle("engine:status", () => engine?.status ?? { state: "starting" });
  ipcMain.handle("file:openSample", async () => {
    try {
      return ok(await openPath(sampleProjectPath(process.resourcesPath), true));
    } catch (err) {
      return fail(err);
    }
  });
  ipcMain.handle("file:open", async () => {
    try {
      const result = await openDialog();
      return result ? ok(result) : null;
    } catch (err) {
      return fail(err);
    }
  });
  ipcMain.handle("file:save", () => saveProject());
  ipcMain.handle("file:saveAs", () => saveProjectAs());
  ipcMain.handle("file:export", (_event, raw: unknown) => {
    const format = ExportFormat.safeParse(raw);
    if (!format.success) return { ok: false, error: { code: "rejected", message: "unknown export format" } };
    return exportFile(format.data);
  });
  ipcMain.handle("file:exportAll", () => exportAll());
  ipcMain.handle("shell:reveal", (_event, raw: unknown) => {
    // Only files this session exported can be revealed.
    if (typeof raw === "string" && session.exported.has(raw)) shell.showItemInFolder(raw);
  });
}

app.whenReady().then(() => {
  const dataDir = path.join(app.getPath("userData"), "engine");
  try {
    engine = new EngineSupervisor(engineBinary(process.resourcesPath), dataDir);
  } catch (err) {
    dialog.showErrorBox("liquidCAD engine missing", err instanceof Error ? err.message : String(err));
    app.quit();
    return;
  }
  engine.on("status", (status) => window?.webContents.send("engine:status", status));
  engine.start();
  registerIpc();
  buildMenu();
  createWindow();
});

app.on("second-instance", () => {
  if (window) {
    if (window.isMinimized()) window.restore();
    window.focus();
  }
});

app.on("activate", () => {
  if (!window && app.isReady()) createWindow();
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") app.quit();
});

app.on("before-quit", () => engine?.stop());
