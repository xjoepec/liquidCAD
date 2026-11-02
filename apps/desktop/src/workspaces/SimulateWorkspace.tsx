// Motion screening: run the Rust wave simulation on the current design and
// play it back in the 3D view.
import { useEffect, useRef, useState } from "react";
import type { SimParams } from "../../shared/ipc";
import { num } from "../format";
import { actions, getState, setState, useApp } from "../store";
import type { SimFrame, SimResult } from "../types";
import { Viewport } from "../viewport/Viewport";

type Series = { key: keyof SimFrame; label: string; unit: string };
const SERIES: Series[] = [
  { key: "heave_in", label: "Heave", unit: "in" },
  { key: "roll_deg", label: "Heel", unit: "deg" },
  { key: "pitch_deg", label: "Trim", unit: "deg" },
  { key: "min_freeboard_in", label: "Min freeboard", unit: "in" },
];

const PRESETS: { label: string; params: Partial<SimParams> }[] = [
  { label: "Boat wake", params: { wave_height_in: 12, period_s: 2.5, heading_deg: 90 } },
  { label: "Chop", params: { wave_height_in: 8, period_s: 1.6, heading_deg: 45 } },
  { label: "Storm swell", params: { wave_height_in: 36, period_s: 4, heading_deg: 0 } },
];

export function SimulateWorkspace() {
  const session = useApp((s) => s.session);
  const sim = useApp((s) => s.sim);
  const simTime = useApp((s) => s.simTime);
  const playing = useApp((s) => s.simPlaying);
  const running = useApp((s) => s.simRunning);
  const [height, setHeight] = useState("12");
  const [period, setPeriod] = useState("2.5");
  const [heading, setHeading] = useState("90");
  const [duration, setDuration] = useState("20");
  const [walkerOn, setWalkerOn] = useState(true);
  const [weight, setWeight] = useState("200");
  const [speed, setSpeed] = useState("40");
  const [rate, setRate] = useState(1);

  // Playback clock.
  const last = useRef<number | null>(null);
  useEffect(() => {
    if (!playing || !sim) {
      last.current = null;
      return;
    }
    let frame = 0;
    const end = sim.frames[sim.frames.length - 1]?.t ?? 0;
    const tick = (now: number) => {
      const prev = last.current ?? now;
      last.current = now;
      const t = getState().simTime + ((now - prev) / 1000) * rate;
      if (t >= end) setState({ simTime: end, simPlaying: false });
      else {
        setState({ simTime: t });
        frame = requestAnimationFrame(tick);
      }
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [playing, sim, rate]);

  if (!session) return <div className="workspace-empty">Open a project to simulate it.</div>;
  const width = session.document.project.parameters.width_in?.value ?? 96;
  const length = session.document.project.parameters.length_in?.value ?? 192;

  const params: SimParams = {
    wave_height_in: Number(height),
    period_s: Number(period),
    heading_deg: Number(heading),
    duration_s: Number(duration),
    walker: walkerOn
      ? { weight_lb: Number(weight), speed_in_s: Number(speed), from: [width * 0.2, length * 0.1], to: [width * 0.8, length * 0.9] }
      : null,
  };
  const valid = [params.wave_height_in, params.period_s, params.heading_deg, params.duration_s].every(Number.isFinite);
  const end = sim?.frames[sim.frames.length - 1]?.t ?? 0;
  const applyPreset = (p: Partial<SimParams>) => {
    if (p.wave_height_in !== undefined) setHeight(String(p.wave_height_in));
    if (p.period_s !== undefined) setPeriod(String(p.period_s));
    if (p.heading_deg !== undefined) setHeading(String(p.heading_deg));
  };

  return (
    <div className="simulate">
      <div className="simulate-view">
        <Viewport />
      </div>
      <aside className="simulate-panel" data-testid="simulate-panel">
        <h2>Waves</h2>
        <div className="seg wrap">
          {PRESETS.map((p) => (
            <button key={p.label} onClick={() => applyPreset(p.params)}>
              {p.label}
            </button>
          ))}
        </div>
        <div className="sim-grid">
          <label>
            Height <input value={height} onChange={(e) => setHeight(e.target.value)} data-testid="sim-height" /> in
          </label>
          <label>
            Period <input value={period} onChange={(e) => setPeriod(e.target.value)} data-testid="sim-period" /> s
          </label>
          <label title="Direction the waves travel: 0 runs stern to bow, 90 port to starboard">
            Heading <input value={heading} onChange={(e) => setHeading(e.target.value)} /> deg
          </label>
          <label>
            Duration <input value={duration} onChange={(e) => setDuration(e.target.value)} data-testid="sim-duration" /> s
          </label>
        </div>
        <label className="check">
          <input type="checkbox" checked={walkerOn} onChange={(e) => setWalkerOn(e.target.checked)} />
          Person walking the deck diagonally
        </label>
        {walkerOn ? (
          <div className="sim-grid">
            <label>
              Weight <input value={weight} onChange={(e) => setWeight(e.target.value)} /> lb
            </label>
            <label>
              Speed <input value={speed} onChange={(e) => setSpeed(e.target.value)} /> in/s
            </label>
          </div>
        ) : null}
        <div className="row-buttons">
          <button className="primary" disabled={!valid || running} onClick={() => void actions.simulate(params)} data-testid="sim-run">
            {running ? "Simulating..." : "Run"}
          </button>
          {sim ? (
            <>
              <button onClick={() => setState({ simPlaying: !playing, simTime: simTime >= end ? 0 : simTime })} data-testid="sim-play">
                {playing ? "Pause" : "Play"}
              </button>
              <select value={rate} onChange={(e) => setRate(Number(e.target.value))} title="Playback speed">
                {[0.25, 0.5, 1, 2, 4].map((r) => (
                  <option key={r} value={r}>
                    {r}x
                  </option>
                ))}
              </select>
            </>
          ) : null}
        </div>
        {sim ? <Results sim={sim} time={simTime} end={end} /> : <p className="hint">Runs heave, roll, and pitch of the dock in regular waves on the current design.</p>}
        <p className="hint small">
          Screening model: one rigid body, per-float buoyancy against the local wave surface, linear damping
          {sim?.coefficients ? ` (${Math.round(sim.coefficients.damping_ratio * 100)}% critical)` : ""} and added mass. Not a
          seakeeping analysis.
        </p>
      </aside>
    </div>
  );
}

function Results({ sim, time, end }: { sim: SimResult; time: number; end: number }) {
  const s = sim.summary;
  return (
    <>
      <input
        type="range"
        className="scrub"
        min={0}
        max={end}
        step={1 / sim.fps}
        value={time}
        onChange={(e) => setState({ simTime: Number(e.target.value), simPlaying: false })}
        data-testid="sim-scrub"
      />
      <div className="sim-time mono">
        t = {num(time, 2)} / {num(end, 1)} s
      </div>
      {SERIES.map((series) => (
        <Spark key={series.key} sim={sim} series={series} time={time} />
      ))}
      <dl className="props" data-testid="sim-summary">
        <dt>Still draft</dt>
        <dd>{num(s.still_draft_in, 2)} in</dd>
        <dt>Wavelength</dt>
        <dd>{num(s.wave_length_in / 12, 1)} ft</dd>
        <dt>Heave period</dt>
        <dd>{num(s.heave_period_s, 2)} s</dd>
        <dt>Max heave</dt>
        <dd>{num(s.max_heave_in, 1)} in</dd>
        <dt>Max heel / trim</dt>
        <dd>
          {num(s.max_roll_deg, 1)} / {num(s.max_pitch_deg, 1)} deg
        </dd>
        <dt>Min freeboard</dt>
        <dd className={s.min_freeboard_in < 0 ? "bad" : ""}>{num(s.min_freeboard_in, 1)} in</dd>
        <dt>Float lift-off</dt>
        <dd>{s.max_float_emergence_in > 0 ? `${num(s.max_float_emergence_in, 1)} in` : "none"}</dd>
        <dt>Deck accel</dt>
        <dd>{num(s.max_deck_accel_g, 2)} g</dd>
      </dl>
      <ul className="sim-checks">
        {sim.diagnostics.length === 0 ? <li className="ok">No green water, slamming, or hard motion.</li> : null}
        {sim.diagnostics.map((d) => (
          <li key={d.code} className={d.severity} data-testid={`sim-${d.code}`}>
            <strong>{d.code}</strong> {d.message}
          </li>
        ))}
      </ul>
    </>
  );
}

function Spark({ sim, series, time }: { sim: SimResult; series: Series; time: number }) {
  const values = sim.frames.map((f) => Number(f[series.key] ?? 0));
  const end = sim.frames[sim.frames.length - 1]?.t ?? 1;
  const lo = Math.min(...values, 0);
  const hi = Math.max(...values, 0);
  const span = hi - lo || 1;
  const W = 260;
  const H = 44;
  const pts = sim.frames.map((f, i) => `${(f.t / end) * W},${H - ((values[i]! - lo) / span) * H}`).join(" ");
  const zero = H - ((0 - lo) / span) * H;
  const current = values[Math.min(values.length - 1, Math.round(time * sim.fps))] ?? 0;
  return (
    <div className="spark">
      <div className="spark-head">
        <span>{series.label}</span>
        <span className="mono">
          {num(current, 2)} {series.unit}
        </span>
      </div>
      <svg
        viewBox={`0 0 ${W} ${H}`}
        preserveAspectRatio="none"
        onPointerDown={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          setState({ simTime: ((e.clientX - r.left) / r.width) * end, simPlaying: false });
        }}
      >
        <line x1={0} x2={W} y1={zero} y2={zero} className="spark-zero" />
        <polyline points={pts} className="spark-line" />
        <line x1={(time / end) * W} x2={(time / end) * W} y1={0} y2={H} className="spark-cursor" />
      </svg>
    </div>
  );
}
