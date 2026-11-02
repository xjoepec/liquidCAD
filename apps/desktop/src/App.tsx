import { useEffect, useState } from "react";
import type { MenuCommand } from "../shared/ipc";
import { bridge } from "./api";
import { BottomPanel } from "./panels/BottomPanel";
import { Inspector } from "./panels/Inspector";
import { ModelTree } from "./panels/ModelTree";
import { actions, getState, handlePushed, log, setState, useApp, type Workspace } from "./store";
import { Viewport } from "./viewport/Viewport";
import { DrawingWorkspace } from "./workspaces/DrawingWorkspace";
import { FabricationWorkspace } from "./workspaces/FabricationWorkspace";
import { SimulateWorkspace } from "./workspaces/SimulateWorkspace";
import { SketchWorkspace } from "./workspaces/SketchWorkspace";

const WORKSPACES: { id: Workspace; label: string }[] = [
  { id: "model", label: "Model" },
  { id: "sketch", label: "Sketch" },
  { id: "drawing", label: "Drawing" },
  { id: "fab", label: "Fabrication" },
  { id: "simulate", label: "Simulate" },
];

function isTyping(): boolean {
  const el = document.activeElement;
  return el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement;
}

function runMenu(command: MenuCommand): void {
  switch (command) {
    case "undo":
      if (isTyping()) document.execCommand("undo");
      else void actions.undo();
      return;
    case "redo":
      if (isTyping()) document.execCommand("redo");
      else void actions.redo();
      return;
    case "view:fit":
      setState((s) => ({ workspace: "model", fitAll: s.fitAll + 1 }));
      return;
    case "view:ortho":
      setState((s) => ({ workspace: "model", ortho: !s.ortho }));
      return;
    case "tool:measure":
      setState({ workspace: "model", tool: "measure" });
      return;
    case "tool:move":
      setState({ workspace: "model", tool: "move" });
      return;
  }
  if (command.startsWith("workspace:")) {
    setState({ workspace: command.slice("workspace:".length) as Workspace });
  } else if (command.startsWith("view:")) {
    const name = command.slice(5) as "iso" | "top" | "front" | "right";
    setState((s) => ({ workspace: "model", view: { name, nonce: s.view.nonce + 1 } }));
  }
}

export function App() {
  const workspace = useApp((s) => s.workspace);
  const hasBridge = typeof window !== "undefined" && Boolean(window.liquidcad);

  useEffect(() => {
    if (!hasBridge) return;
    const api = bridge();
    void api.status().then((status) => setState({ engine: status }));
    const offStatus = api.onStatus((status) => {
      setState({ engine: status });
      if (status.state === "restarting") log("error", `engine ${status.reason}; restarting`);
      if (status.state === "failed") log("error", `engine failed: ${status.reason}`);
    });
    const offMenu = api.onMenu(runMenu);
    const offPushed = api.onPushed(handlePushed);
    return () => {
      offStatus();
      offMenu();
      offPushed();
    };
  }, [hasBridge]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (isTyping() || e.metaKey || e.ctrlKey || e.altKey) return;
      const s = getState();
      switch (e.key) {
        case "Escape":
          if (s.tool !== "select") setState({ tool: "select" });
          else if (s.picks.length > 0) setState({ picks: [] });
          else void actions.select(null);
          break;
        case "f":
          runMenu("view:fit");
          break;
        case "m":
          runMenu("tool:measure");
          break;
        case "g":
          runMenu("tool:move");
          break;
        case "o":
          runMenu("view:ortho");
          break;
        case "t":
          runMenu("view:top");
          break;
        case "i":
          runMenu("view:iso");
          break;
        default:
          return;
      }
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  if (!hasBridge) {
    return (
      <div className="no-bridge">
        <h1>liquidCAD</h1>
        <p>This interface runs inside the desktop app. Start it with <code>npm run dev</code> in apps/desktop.</p>
      </div>
    );
  }

  return (
    <div className="app">
      <Toolbar />
      <div className="main">
        <ModelTree />
        <main className="center">
          {workspace === "model" ? <Viewport /> : null}
          {workspace === "sketch" ? <SketchWorkspace /> : null}
          {workspace === "drawing" ? <DrawingWorkspace /> : null}
          {workspace === "fab" ? <FabricationWorkspace /> : null}
          {workspace === "simulate" ? <SimulateWorkspace /> : null}
        </main>
        <Inspector />
      </div>
      <BottomPanel />
      <StatusBar />
    </div>
  );
}

function Toolbar() {
  const session = useApp((s) => s.session);
  const workspace = useApp((s) => s.workspace);
  const [exportOpen, setExportOpen] = useState(false);
  return (
    <header className="toolbar">
      <div className="brand">
        liquid<span>CAD</span>
      </div>
      <div className="seg">
        <button className="primary" onClick={() => void actions.openSample()} data-testid="open-sample">
          Open modular dock
        </button>
        <button onClick={() => void actions.openProject()}>Open...</button>
        <button onClick={() => void actions.save()} disabled={!session}>
          Save
        </button>
        <button onClick={() => void actions.save(true)} disabled={!session}>
          Save as...
        </button>
      </div>
      <div className="seg dropdown">
        <button onClick={() => setExportOpen((o) => !o)} disabled={!session} data-testid="export-menu">
          Export
        </button>
        {exportOpen && session ? (
          <div className="menu" onMouseLeave={() => setExportOpen(false)}>
            {(
              [
                ["dxf", "DXF fabrication plan"],
                ["step", "STEP AP214 solids"],
                ["pdf", "PDF drawing sheet"],
                ["csv", "CSV bill of materials"],
              ] as const
            ).map(([format, label]) => (
              <button
                key={format}
                data-testid={`export-${format}`}
                onClick={() => {
                  setExportOpen(false);
                  void actions.exportFile(format);
                }}
              >
                {label}
              </button>
            ))}
            <button
              onClick={() => {
                setExportOpen(false);
                void actions.exportAll();
              }}
            >
              All four to folder...
            </button>
          </div>
        ) : null}
      </div>
      <div className="seg">
        <button
          onClick={() => void actions.undo()}
          disabled={!session?.can_undo}
          title={session?.undo_label ? `Undo ${session.undo_label}` : "Nothing to undo"}
          data-testid="undo"
        >
          Undo
        </button>
        <button
          onClick={() => void actions.redo()}
          disabled={!session?.can_redo}
          title={session?.redo_label ? `Redo ${session.redo_label}` : "Nothing to redo"}
          data-testid="redo"
        >
          Redo
        </button>
      </div>
      <nav className="workspaces">
        {WORKSPACES.map((w) => (
          <button
            key={w.id}
            className={workspace === w.id ? "on" : ""}
            onClick={() => setState({ workspace: w.id })}
            data-testid={`workspace-${w.id}`}
          >
            {w.label}
          </button>
        ))}
      </nav>
    </header>
  );
}

function StatusBar() {
  const engine = useApp((s) => s.engine);
  const session = useApp((s) => s.session);
  const pending = useApp((s) => s.pending);
  const selectedId = useApp((s) => s.selectedId);
  const engineText =
    engine.state === "ready"
      ? engine.kernel
      : engine.state === "starting"
        ? "engine starting"
        : engine.state === "restarting"
          ? `engine restarting (${engine.reason})`
          : `engine failed: ${engine.reason}`;
  return (
    <footer className="statusbar" data-testid="statusbar">
      <span className={`engine ${engine.state}`} data-testid="engine-state" data-state={engine.state}>
        <span className="dot" /> {engineText}
      </span>
      {session ? (
        <>
          <span data-testid="revision">rev {session.document.revision}</span>
          <span>
            {pending > 0
              ? "regenerating..."
              : `regenerated ${session.rebuild.regenerated.length}, reused ${session.rebuild.reused.length} features in ${session.rebuild.elapsed_ms.toFixed(1)} ms`}
          </span>
          <span>
            meshes built {session.meshes_built}, reused {session.meshes_reused}
          </span>
          {selectedId ? <span className="mono">{selectedId}</span> : null}
          <span className="path" title={session.project_path ?? ""}>
            {session.dirty ? "* " : ""}
            {session.project_path ?? "unsaved"}
          </span>
        </>
      ) : (
        <span>no project</span>
      )}
    </footer>
  );
}
