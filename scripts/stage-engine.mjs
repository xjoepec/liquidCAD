#!/usr/bin/env node
// Stages the release engine for packaging into target/stage/bin.
//
// Without OpenCascade the engine is a single static binary and is copied as
// is. With OpenCascade linked it depends on OCCT's shared libraries through
// an absolute rpath into the build machine's OCCT install; those libraries
// are copied next to it (bin/lib) and the engine is pointed at them, so the
// packaged app runs on machines that never installed OCCT.
//
//   macOS:   libraries resolved with otool, rpath rewritten with
//            install_name_tool, engine re-signed ad hoc (required on arm64).
//   Linux:   libraries resolved with ldd, rpath set to $ORIGIN/lib with
//            patchelf when available.
//   Windows: DLLs from the OCCT prefix are copied next to the .exe.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const exe = process.platform === "win32" ? "liquidcad.exe" : "liquidcad";
const source = path.join(root, "target", "release", exe);
const out = path.join(root, "target", "stage", "bin");
const libDir = path.join(out, "lib");

function tool(name) {
  const clt = `/Library/Developer/CommandLineTools/usr/bin/${name}`;
  return process.platform === "darwin" && fs.existsSync(clt) ? clt : name;
}

function run(cmd, args) {
  return execFileSync(cmd, args, { encoding: "utf8" });
}

if (!fs.existsSync(source)) {
  console.error(`missing ${source}; run cargo build --release -p liquidcad-cli first`);
  process.exit(1);
}
fs.rmSync(out, { recursive: true, force: true });
fs.mkdirSync(out, { recursive: true });
const engine = path.join(out, exe);
fs.copyFileSync(source, engine);
fs.chmodSync(engine, 0o755);

if (process.platform === "darwin") {
  const otool = tool("otool");
  const rpaths = [...run(otool, ["-l", engine]).matchAll(/cmd LC_RPATH[\s\S]*?path (.+?) \(offset/g)].map((m) => m[1]);
  const occt = rpaths.find((p) => fs.existsSync(path.join(p, "libTKernel.dylib")) || fs.existsSync(p));
  if (!occt) {
    console.log("engine has no OpenCascade rpath; staged the binary alone");
  } else {
    fs.mkdirSync(libDir, { recursive: true });
    const seen = new Set();
    const todo = [engine];
    while (todo.length > 0) {
      const file = todo.pop();
      for (const line of run(otool, ["-L", file]).split("\n").slice(1)) {
        const name = line.trim().split(" (")[0];
        if (!name?.startsWith("@rpath/")) continue;
        const base = name.slice("@rpath/".length);
        if (seen.has(base)) continue;
        const from = path.join(occt, base);
        if (!fs.existsSync(from)) throw new Error(`cannot find ${base} in ${occt}`);
        seen.add(base);
        fs.copyFileSync(fs.realpathSync(from), path.join(libDir, base));
        todo.push(path.join(libDir, base));
      }
    }
    const installNameTool = tool("install_name_tool");
    for (const p of rpaths) run(installNameTool, ["-delete_rpath", p, engine]);
    run(installNameTool, ["-add_rpath", "@executable_path/lib", engine]);
    execFileSync("codesign", ["--force", "--sign", "-", engine], { stdio: "inherit" });
    const bytes = [...seen].reduce((n, b) => n + fs.statSync(path.join(libDir, b)).size, 0);
    console.log(`staged engine with ${seen.size} OpenCascade libraries (${(bytes / 1e6).toFixed(1)} MB) in bin/lib`);
  }
} else if (process.platform === "linux") {
  const deps = run("ldd", [engine])
    .split("\n")
    .map((l) => l.match(/=>\s+(\S+)/)?.[1])
    .filter((p) => p && /libTK|\/\.occt\//.test(p));
  if (deps.length > 0) {
    fs.mkdirSync(libDir, { recursive: true });
    for (const dep of deps) fs.copyFileSync(fs.realpathSync(dep), path.join(libDir, path.basename(dep)));
    try {
      run("patchelf", ["--set-rpath", "$ORIGIN/lib", engine]);
      console.log(`staged engine with ${deps.length} OpenCascade libraries`);
    } catch {
      console.warn("patchelf not found: set LD_LIBRARY_PATH to bin/lib when running the packaged engine");
    }
  } else {
    console.log("engine has no OpenCascade libraries; staged the binary alone");
  }
} else if (process.platform === "win32") {
  const prefix = process.env.OCCT_ROOT ?? path.join(root, ".occt");
  const bin = [path.join(prefix, "Library", "bin"), path.join(prefix, "win64", "vc14", "bin")].find((d) => fs.existsSync(d));
  if (bin) {
    let n = 0;
    for (const f of fs.readdirSync(bin)) {
      if (f.toLowerCase().endsWith(".dll")) {
        fs.copyFileSync(path.join(bin, f), path.join(out, f));
        n++;
      }
    }
    console.log(`staged engine with ${n} DLLs from ${bin}`);
  } else {
    console.log("no OpenCascade DLL folder found; staged the binary alone");
  }
}
