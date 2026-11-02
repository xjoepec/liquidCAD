import react from "@vitejs/plugin-react";
import { defineConfig, type Plugin } from "vitest/config";

// The production CSP forbids inline scripts. Vite's dev server injects the
// React refresh preamble inline and talks over a websocket, so relax the
// policy only while serving.
function devCsp(): Plugin {
  return {
    name: "liquidcad-dev-csp",
    apply: "serve",
    transformIndexHtml(html) {
      return html
        .replace("script-src 'self'", "script-src 'self' 'unsafe-inline'")
        .replace("connect-src 'self'", "connect-src 'self' ws://127.0.0.1:5173 http://127.0.0.1:5173");
    },
  };
}

export default defineConfig({
  plugins: [react(), devCsp()],
  base: "./",
  server: {
    host: "127.0.0.1",
    port: 5173,
    strictPort: true,
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    chunkSizeWarningLimit: 1200,
  },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.ts", "src/**/*.test.tsx", "shared/**/*.test.ts"],
  },
});
