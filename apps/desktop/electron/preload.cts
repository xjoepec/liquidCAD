// Sandboxed preload. Compiled to CommonJS (preload.cjs) because sandboxed
// preloads cannot load ES modules or local files. It only forwards to named
// channels; the main process validates every payload.
import { contextBridge, ipcRenderer, type IpcRendererEvent } from "electron";
import type { EngineStatus, IpcResult, LiquidCadApi, MenuCommand } from "../shared/ipc.js";

const api: LiquidCadApi = {
  request: (request) => ipcRenderer.invoke("engine:request", request),
  status: () => ipcRenderer.invoke("engine:status"),
  openSample: () => ipcRenderer.invoke("file:openSample"),
  openProject: () => ipcRenderer.invoke("file:open"),
  saveProject: () => ipcRenderer.invoke("file:save"),
  saveProjectAs: () => ipcRenderer.invoke("file:saveAs"),
  exportFile: (format) => ipcRenderer.invoke("file:export", format),
  exportAll: () => ipcRenderer.invoke("file:exportAll"),
  reveal: (path) => ipcRenderer.invoke("shell:reveal", path),
  onMenu: (handler) => {
    const listener = (_event: IpcRendererEvent, command: MenuCommand) => handler(command);
    ipcRenderer.on("menu:command", listener);
    return () => ipcRenderer.removeListener("menu:command", listener);
  },
  onStatus: (handler) => {
    const listener = (_event: IpcRendererEvent, status: EngineStatus) => handler(status);
    ipcRenderer.on("engine:status", listener);
    return () => ipcRenderer.removeListener("engine:status", listener);
  },
  onPushed: (handler) => {
    const listener = (_event: IpcRendererEvent, result: IpcResult<unknown>) => handler(result);
    ipcRenderer.on("engine:pushed", listener);
    return () => ipcRenderer.removeListener("engine:pushed", listener);
  },
};

contextBridge.exposeInMainWorld("liquidcad", api);
