// Supervises the Rust engine process (`liquidcad serve`) and speaks its
// JSON-lines protocol. The engine is restarted if it dies unexpectedly.
import { ChildProcessWithoutNullStreams, spawn } from "node:child_process";
import { EventEmitter } from "node:events";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import type { EngineStatus } from "../shared/ipc.js";

const here = path.dirname(fileURLToPath(import.meta.url));
// dist-electron/electron -> apps/desktop -> repo root
const repoRoot = path.resolve(here, "../../../..");

export type EngineRequest = { method: string; params?: unknown };
export type EngineReply = { id: number; result?: { type: string; data?: unknown }; error?: { code: string; message: string } };

export class EngineError extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

export function engineBinary(resourcesPath?: string): string {
  if (process.env.LIQUIDCAD_ENGINE) return process.env.LIQUIDCAD_ENGINE;
  const exe = process.platform === "win32" ? "liquidcad.exe" : "liquidcad";
  // A packaged app always runs the engine (and OCCT libraries) it ships with.
  const bundled = resourcesPath ? path.join(resourcesPath, "bin", exe) : "";
  if (bundled && fs.existsSync(bundled)) return bundled;
  const candidates = [path.join(repoRoot, "target", "release", exe), path.join(repoRoot, "target", "debug", exe)].filter((p) =>
    fs.existsSync(p),
  );
  if (candidates.length === 0) {
    throw new EngineError(
      "engine_missing",
      `liquidCAD engine not found. Build it with "cargo build -p liquidcad-cli" or set LIQUIDCAD_ENGINE.`,
    );
  }
  // Prefer whichever build is newest so a stale release binary never shadows a fresh debug build.
  return candidates.sort((a, b) => fs.statSync(b).mtimeMs - fs.statSync(a).mtimeMs)[0]!;
}

export function sampleProjectPath(resourcesPath?: string): string {
  const fromRepo = path.join(repoRoot, "projects", "modular-dock", "liquidcad.json");
  if (fs.existsSync(fromRepo)) return fromRepo;
  return path.join(resourcesPath ?? "", "projects", "modular-dock", "liquidcad.json");
}

type Pending = { resolve: (r: EngineReply) => void; timer: NodeJS.Timeout };

export class EngineSupervisor extends EventEmitter {
  private child: ChildProcessWithoutNullStreams | null = null;
  private buffer = "";
  private nextId = 1;
  private pending = new Map<number, Pending>();
  private restarts = 0;
  private stopping = false;
  private readyWaiters: Array<(ok: boolean) => void> = [];
  status: EngineStatus = { state: "starting" };

  constructor(
    private readonly binary: string,
    private readonly dataDir: string,
    private readonly timeoutMs = 120_000,
  ) {
    super();
  }

  start(): void {
    this.stopping = false;
    this.setStatus({ state: "starting" });
    const child = spawn(this.binary, ["serve", "--data-dir", this.dataDir], {
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    });
    this.child = child;
    this.buffer = "";
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk: string) => this.onData(chunk));
    child.stderr.setEncoding("utf8");
    child.stderr.on("data", (chunk: string) => process.stderr.write(`[engine] ${chunk}`));
    child.on("error", (err) => this.onExit(`spawn failed: ${err.message}`));
    child.on("exit", (code, signal) => this.onExit(`exited with ${signal ?? code}`));
  }

  /** Resolves once the engine has announced itself, or false on failure. */
  ready(timeoutMs = 15_000): Promise<boolean> {
    if (this.status.state === "ready") return Promise.resolve(true);
    if (this.status.state === "failed") return Promise.resolve(false);
    return new Promise((resolve) => {
      const timer = setTimeout(() => resolve(false), timeoutMs);
      this.readyWaiters.push((ok) => {
        clearTimeout(timer);
        resolve(ok);
      });
    });
  }

  async call(request: EngineRequest): Promise<{ type: string; data?: unknown }> {
    if (!(await this.ready())) {
      throw new EngineError("engine_unavailable", `engine is ${this.status.state}`);
    }
    const child = this.child;
    if (!child) throw new EngineError("engine_unavailable", "engine is not running");
    const id = this.nextId++;
    const reply = await new Promise<EngineReply>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new EngineError("timeout", `engine did not answer ${request.method} in ${this.timeoutMs} ms`));
      }, this.timeoutMs);
      this.pending.set(id, { resolve, timer });
      child.stdin.write(`${JSON.stringify({ id, payload: request })}\n`, (err) => {
        if (err) {
          clearTimeout(timer);
          this.pending.delete(id);
          reject(new EngineError("engine_io", err.message));
        }
      });
    });
    if (reply.error) throw new EngineError(reply.error.code, reply.error.message);
    if (!reply.result) throw new EngineError("protocol", "engine reply had neither result nor error");
    return reply.result;
  }

  stop(): void {
    this.stopping = true;
    this.child?.stdin.end();
    const child = this.child;
    setTimeout(() => child?.kill(), 1500).unref();
  }

  private onData(chunk: string): void {
    this.buffer += chunk;
    let newline: number;
    while ((newline = this.buffer.indexOf("\n")) >= 0) {
      const line = this.buffer.slice(0, newline).trim();
      this.buffer = this.buffer.slice(newline + 1);
      if (!line) continue;
      let message: Record<string, unknown>;
      try {
        message = JSON.parse(line);
      } catch {
        process.stderr.write(`[engine] unparseable line: ${line.slice(0, 200)}\n`);
        continue;
      }
      if (message.event === "ready") {
        this.restarts = 0;
        this.setStatus({
          state: "ready",
          kernel: String(message.kernel),
          nativeOcct: Boolean(message.native_occt),
          version: String(message.version),
        });
        continue;
      }
      const reply = message as unknown as EngineReply;
      const waiter = this.pending.get(reply.id);
      if (waiter) {
        clearTimeout(waiter.timer);
        this.pending.delete(reply.id);
        waiter.resolve(reply);
      }
    }
  }

  private onExit(reason: string): void {
    if (!this.child) return;
    this.child = null;
    for (const [id, waiter] of this.pending) {
      clearTimeout(waiter.timer);
      waiter.resolve({ id, error: { code: "engine_exit", message: `engine ${reason}` } });
    }
    this.pending.clear();
    if (this.stopping) return;
    if (this.restarts >= 3) {
      this.setStatus({ state: "failed", reason });
      return;
    }
    this.restarts += 1;
    this.setStatus({ state: "restarting", reason });
    setTimeout(() => this.start(), 250 * this.restarts);
  }

  private setStatus(status: EngineStatus): void {
    this.status = status;
    if (status.state === "ready" || status.state === "failed") {
      const waiters = this.readyWaiters;
      this.readyWaiters = [];
      for (const w of waiters) w(status.state === "ready");
    }
    this.emit("status", status);
  }
}
