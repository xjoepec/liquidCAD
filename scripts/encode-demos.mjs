#!/usr/bin/env node
// Cuts artifacts/videos/e2e.webm (from npm run demo) into the README clips in
// docs/media, using the per-test start/end marks in artifacts/videos/e2e.json.
// Each clip is one or more test segments, joined in the order listed:
//   <name>.mp4  H.264, full resolution, for downloading or playing locally
//   <name>.gif  1000 px wide, 12 fps, for the README (GitHub plays GIFs inline)
// Clips play at SPEED times the recorded rate, and the last frame is held so
// the result can be read. Needs ffmpeg on PATH.
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const src = path.join(root, "artifacts", "videos");
const out = path.join(root, "docs", "media");

const SPEED = 3;
const HOLD = 1.5; // seconds the final frame stays up, after speed-up
const FPS = 25; // Playwright's recording rate

// Clip name -> test title prefixes from apps/desktop/e2e/desktop.spec.ts.
const CLIPS = [
  ["01-open-and-resize", ["1-4:", "5:", "6-9:"]],
  ["02-materials-and-undo", ["materials drive diagnostics"]],
  ["03-move-a-cleat", ["cleats move through the engine"]],
  ["04-pick-and-measure", ["faces and edges are picked"]],
  ["05-sketch-a-part", ["a part sketched on a deck board"]],
  ["06-fillet-an-edge", ["edges take fillets"]],
  ["07-gangway-hinge", ["the gangway slope swings"]],
  ["08-wave-simulation", ["wave simulation runs"]],
  ["09-drawings-and-exports", ["sketch, drawing and fabrication", "10: exports real"]],
];

const video = path.join(src, "e2e.webm");
const marksFile = path.join(src, "e2e.json");
if (!fs.existsSync(video) || !fs.existsSync(marksFile)) {
  console.error(`no recording in ${src}; run npm run demo first`);
  process.exit(1);
}
const marks = JSON.parse(fs.readFileSync(marksFile, "utf8"));
fs.mkdirSync(out, { recursive: true });

const ff = (args) => execFileSync("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y", ...args], { stdio: "inherit" });
const segment = (prefix) => {
  const m = marks.find((x) => x.title.startsWith(prefix));
  if (!m || m.end == null) throw new Error(`no complete mark for test "${prefix}" in ${marksFile}`);
  return m;
};

for (const [name, prefixes] of CLIPS) {
  const segs = prefixes.map(segment);
  // Trim each test's span, join them, speed up, hold the end.
  const trims = segs
    .map((s, i) => `[0:v]trim=start=${s.start.toFixed(3)}:end=${s.end.toFixed(3)},setpts=PTS-STARTPTS[s${i}]`)
    .join(";");
  const joined = segs.map((_, i) => `[s${i}]`).join("") + `concat=n=${segs.length}:v=1:a=0`;
  const pace = `setpts=PTS/${SPEED},fps=${FPS},tpad=stop_mode=clone:stop_duration=${HOLD}`;
  const base = `${trims};${joined},${pace}`;
  const mp4 = path.join(out, `${name}.mp4`);
  const gif = path.join(out, `${name}.gif`);
  ff(["-i", video, "-filter_complex", `${base}[v]`, "-map", "[v]", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "22", "-preset", "slow", "-movflags", "+faststart", mp4]);
  ff([
    "-i",
    mp4,
    "-vf",
    "fps=12,scale=1000:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=160:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle",
    gif,
  ]);
  const size = (f) => (fs.statSync(f).size / 1e6).toFixed(1);
  const secs = Number(execFileSync("ffprobe", ["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", mp4]).toString()).toFixed(1);
  console.log(`${name}: ${secs}s, mp4 ${size(mp4)} MB, gif ${size(gif)} MB`);
}
