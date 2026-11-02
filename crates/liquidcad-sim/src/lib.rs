//! Time-domain screening of a floating dock in regular waves.
//!
//! The dock is one rigid body with three degrees of freedom: heave (z),
//! roll (about x, through the centre of gravity), and pitch (about y). Each
//! float is a box; at every step its submerged depth is measured against
//! the wave surface at the float's centre, so buoyancy, its moments, and
//! float emergence follow the real float layout rather than a waterplane
//! idealisation.
//!
//! Waves are regular deep-water Airy waves, η = A cos(k d·p − ωt) with
//! ω² = g k. Added mass and damping are linear, with fixed fractions stated
//! on [`Coefficients`]. This is a screening model for comparing designs and
//! spotting green water or slamming. It is not a seakeeping analysis: it
//! ignores radiation and diffraction, the pressure decay with depth, and
//! surge, sway, and yaw.

use liquidcad_core::diagnostics::{Diagnostic, Severity};
use liquidcad_core::geometry::{MassProps, Mat3, Vec3};
use serde::{Deserialize, Serialize};

const G: f64 = 9.80665; // m/s²
const RHO: f64 = 1000.0; // kg/m³, fresh water (matches the 62.4 lb/ft³ used in hydrostatics)
const M_PER_IN: f64 = 0.0254;
const KG_PER_LB: f64 = 0.453_592_37;
/// 240 Hz: an exact multiple of the 30 fps output, so frames are evenly spaced.
const DT: f64 = 1.0 / 240.0;
const FPS: f64 = 30.0;

pub const MAX_DURATION_S: f64 = 120.0;

/// Linear hydrodynamic coefficients. Fractions of the rigid-body value.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Coefficients {
    /// Heave added mass as a fraction of the floating mass.
    pub added_mass_heave: f64,
    /// Roll and pitch added inertia as a fraction of the body's inertia.
    pub added_inertia: f64,
    /// Damping ratio (fraction of critical) in every mode.
    pub damping_ratio: f64,
}

impl Default for Coefficients {
    fn default() -> Self {
        Self {
            added_mass_heave: 0.5,
            added_inertia: 0.25,
            damping_ratio: 0.1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Walker {
    pub weight_lb: f64,
    /// Plan positions, inches; the walker goes from `from` to `to` and back.
    pub from: [f64; 2],
    pub to: [f64; 2],
    pub speed_in_s: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimParams {
    pub wave_height_in: f64,
    pub period_s: f64,
    /// Direction the waves travel, degrees from +y (stern to bow),
    /// clockwise toward +x.
    pub heading_deg: f64,
    pub duration_s: f64,
    #[serde(default)]
    pub walker: Option<Walker>,
}

impl SimParams {
    pub fn validate(&self) -> Result<(), String> {
        let finite = [
            self.wave_height_in,
            self.period_s,
            self.heading_deg,
            self.duration_s,
        ]
        .iter()
        .all(|v| v.is_finite());
        if !finite {
            return Err("simulation inputs must be finite".into());
        }
        if !(0.0..=120.0).contains(&self.wave_height_in) {
            return Err("wave height must be 0 to 120 in".into());
        }
        if !(1.0..=20.0).contains(&self.period_s) {
            return Err("wave period must be 1 to 20 s".into());
        }
        if !(1.0..=MAX_DURATION_S).contains(&self.duration_s) {
            return Err(format!("duration must be 1 to {MAX_DURATION_S} s"));
        }
        if let Some(w) = &self.walker {
            let ok = [
                w.weight_lb,
                w.speed_in_s,
                w.from[0],
                w.from[1],
                w.to[0],
                w.to[1],
            ]
            .iter()
            .all(|v| v.is_finite());
            if !ok
                || !(0.0..=600.0).contains(&w.weight_lb)
                || !(1.0..=120.0).contains(&w.speed_in_s)
            {
                return Err("walker weight must be 0 to 600 lb and speed 1 to 120 in/s".into());
            }
        }
        Ok(())
    }
}

/// One float as an axis-aligned box in the dock's model frame (inches,
/// z from the float bottoms).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FloatBox {
    pub min: Vec3,
    pub size: Vec3,
}

/// What floats and how it is loaded.
#[derive(Clone, Debug)]
pub struct Body {
    /// Mass carried by the floats (lb), its centre of gravity and inertia.
    pub mass: MassProps,
    pub floats: Vec<FloatBox>,
    /// Deck top height and plan corners for freeboard.
    pub deck_top_in: f64,
    pub deck_corners: [[f64; 2]; 4],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub t: f64,
    /// Rise of the centre of gravity from its still-water rest, inches.
    pub heave_in: f64,
    /// Heel, positive with starboard down.
    pub roll_deg: f64,
    /// Trim, positive with the bow down.
    pub pitch_deg: f64,
    /// Lowest deck corner height above the local water surface, inches.
    pub min_freeboard_in: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub walker: Option<[f64; 2]>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub still_draft_in: f64,
    pub static_heel_deg: f64,
    pub static_trim_deg: f64,
    pub heave_period_s: f64,
    pub max_heave_in: f64,
    pub max_roll_deg: f64,
    pub max_pitch_deg: f64,
    pub min_freeboard_in: f64,
    /// Largest gap between a float bottom and the water surface.
    pub max_float_emergence_in: f64,
    pub max_deck_accel_g: f64,
    pub wave_length_in: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SimResult {
    pub params: Option<SimParams>,
    pub fps: f64,
    /// Centre of gravity in the model frame, inches: roll and pitch turn
    /// the model about this point.
    pub pivot: Vec3,
    /// Still-water level in the model frame at rest, inches.
    pub water_level_in: f64,
    pub frames: Vec<Frame>,
    pub summary: Summary,
    pub diagnostics: Vec<Diagnostic>,
    pub coefficients: Option<Coefficients>,
}

/// Wave field in model coordinates (metres internally).
struct Wave {
    amp: f64,
    k: f64,
    omega: f64,
    dir: [f64; 2],
    ramp: f64,
}

impl Wave {
    fn new(p: &SimParams) -> Self {
        let omega = 2.0 * std::f64::consts::PI / p.period_s;
        let h = p.heading_deg.to_radians();
        Self {
            amp: p.wave_height_in * 0.5 * M_PER_IN,
            k: omega * omega / G,
            omega,
            dir: [h.sin(), h.cos()],
            ramp: p.period_s * 2.0,
        }
    }

    /// Surface elevation at plan point `x, y` (metres) and time `t`.
    fn eta(&self, x: f64, y: f64, t: f64) -> f64 {
        let fade = if t < self.ramp {
            0.5 - 0.5 * (std::f64::consts::PI * t / self.ramp).cos()
        } else {
            1.0
        };
        fade * self.amp * (self.k * (self.dir[0] * x + self.dir[1] * y) - self.omega * t).cos()
    }
}

/// Body in SI, relative to its centre of gravity.
struct Model {
    mass: f64,
    m_eff: f64,
    i_roll: f64,
    i_pitch: f64,
    floats: Vec<(f64, f64, f64, f64)>, // (x, y relative to CG; area; height)
    float_bottom: f64,                 // float bottom z relative to CG at zero heave
    cg_plan: [f64; 2],                 // CG plan position (absolute, for wave phase)
    corners: Vec<[f64; 2]>,            // relative to CG
    deck_rel: f64,                     // deck top relative to CG
    c: [f64; 3],
}

/// `phi` turns about x (the dock's transverse axis: pitch, positive lifts
/// the bow); `theta` turns about y (longitudinal: roll, positive lowers
/// starboard).
#[derive(Clone, Copy, Default)]
struct State {
    z: f64,
    phi: f64,
    theta: f64,
    vz: f64,
    vphi: f64,
    vtheta: f64,
}

impl Model {
    /// Vertical displacement of a body point (relative plan position).
    fn dz(s: &State, x: f64, y: f64) -> f64 {
        s.z + s.phi * y - s.theta * x
    }

    /// Net vertical force and roll, pitch moments (SI), plus the smallest
    /// submerged depth ratio seen (for emergence).
    fn loads(&self, s: &State, wave: &Wave, t: f64, walker: Option<(f64, [f64; 2])>) -> [f64; 3] {
        let mut f = -self.mass * G;
        let (mut mx, mut my) = (0.0, 0.0);
        for &(x, y, area, height) in &self.floats {
            let bottom = self.float_bottom + Self::dz(s, x, y);
            let surface = wave.eta(self.cg_plan[0] + x, self.cg_plan[1] + y, t);
            let depth = (surface - bottom).clamp(0.0, height);
            let b = RHO * G * area * depth;
            f += b;
            mx += b * y;
            my -= b * x;
        }
        if let Some((weight, [x, y])) = walker {
            f -= weight;
            mx -= weight * y;
            my += weight * x;
        }
        [f, mx, my]
    }

    fn derivative(&self, s: &State, wave: &Wave, t: f64, walker: Option<(f64, [f64; 2])>) -> State {
        let [f, mx, my] = self.loads(s, wave, t, walker);
        State {
            z: s.vz,
            phi: s.vphi,
            theta: s.vtheta,
            vz: (f - self.c[0] * s.vz) / self.m_eff,
            vphi: (mx - self.c[1] * s.vphi) / self.i_roll,
            vtheta: (my - self.c[2] * s.vtheta) / self.i_pitch,
        }
    }
}

fn add(s: &State, d: &State, h: f64) -> State {
    State {
        z: s.z + d.z * h,
        phi: s.phi + d.phi * h,
        theta: s.theta + d.theta * h,
        vz: s.vz + d.vz * h,
        vphi: s.vphi + d.vphi * h,
        vtheta: s.vtheta + d.vtheta * h,
    }
}

fn walker_at(w: &Walker, t: f64) -> [f64; 2] {
    let dx = w.to[0] - w.from[0];
    let dy = w.to[1] - w.from[1];
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-9 {
        return w.from;
    }
    let travel = (w.speed_in_s * t) % (2.0 * len);
    let s = if travel <= len {
        travel
    } else {
        2.0 * len - travel
    } / len;
    [w.from[0] + dx * s, w.from[1] + dy * s]
}

/// Runs the simulation.
pub fn simulate(body: &Body, params: &SimParams) -> Result<SimResult, String> {
    simulate_with(body, params, Coefficients::default())
}

pub fn simulate_with(
    body: &Body,
    params: &SimParams,
    coef: Coefficients,
) -> Result<SimResult, String> {
    params.validate()?;
    if body.floats.is_empty() {
        return Err("nothing floats: the dock has no floats".into());
    }
    let m = body.mass.mass_lb * KG_PER_LB;
    if m <= 0.0 {
        return Err("the dock has no mass".into());
    }
    let cg = body.mass.com;
    let in_m = |v: f64| v * M_PER_IN;
    // Inertia about the CG, lb·in² -> kg·m².
    let inertia: Mat3 = body.mass.inertia;
    let scale = KG_PER_LB * M_PER_IN * M_PER_IN;
    let floats: Vec<(f64, f64, f64, f64)> = body
        .floats
        .iter()
        .map(|f| {
            let cx = f.min[0] + f.size[0] * 0.5 - cg[0];
            let cy = f.min[1] + f.size[1] * 0.5 - cg[1];
            (
                in_m(cx),
                in_m(cy),
                in_m(f.size[0]) * in_m(f.size[1]),
                in_m(f.size[2]),
            )
        })
        .collect();
    let bottom = body
        .floats
        .iter()
        .map(|f| f.min[2])
        .fold(f64::INFINITY, f64::min);
    let area: f64 = floats.iter().map(|f| f.2).sum();
    let mut model = Model {
        mass: m,
        m_eff: m * (1.0 + coef.added_mass_heave),
        i_roll: inertia[0][0] * scale * (1.0 + coef.added_inertia),
        i_pitch: inertia[1][1] * scale * (1.0 + coef.added_inertia),
        floats,
        float_bottom: in_m(bottom - cg[2]),
        cg_plan: [in_m(cg[0]), in_m(cg[1])],
        corners: body
            .deck_corners
            .iter()
            .map(|c| [in_m(c[0] - cg[0]), in_m(c[1] - cg[1])])
            .collect(),
        deck_rel: in_m(body.deck_top_in - cg[2]),
        c: [0.0; 3],
    };
    // Hydrostatic stiffness for damping (and the natural period).
    let k_heave = RHO * G * area;
    let k_roll: f64 = model.floats.iter().map(|f| RHO * G * f.2 * f.1 * f.1).sum();
    let k_pitch: f64 = model.floats.iter().map(|f| RHO * G * f.2 * f.0 * f.0).sum();
    model.c = [
        2.0 * coef.damping_ratio * (k_heave * model.m_eff).sqrt(),
        2.0 * coef.damping_ratio * (k_roll * model.i_roll).sqrt(),
        2.0 * coef.damping_ratio * (k_pitch * model.i_pitch).sqrt(),
    ];

    // Static equilibrium in still water: Newton on (force, moments) = 0.
    let calm = Wave {
        amp: 0.0,
        k: 0.0,
        omega: 0.0,
        dir: [0.0, 1.0],
        ramp: 1.0,
    };
    let mut rest = State {
        z: -model.float_bottom - m / (RHO * area),
        ..State::default()
    };
    for _ in 0..30 {
        let r = model.loads(&rest, &calm, 0.0, None);
        if r.iter().map(|v| v.abs()).sum::<f64>() < 1e-6 {
            break;
        }
        let mut jac = [[0.0; 3]; 3];
        let h = [1e-6, 1e-7, 1e-7];
        for j in 0..3 {
            let mut p = rest;
            match j {
                0 => p.z += h[j],
                1 => p.phi += h[j],
                _ => p.theta += h[j],
            }
            let rp = model.loads(&p, &calm, 0.0, None);
            for i in 0..3 {
                jac[i][j] = (rp[i] - r[i]) / h[j];
            }
        }
        let Some(step) = solve3(jac, [-r[0], -r[1], -r[2]]) else {
            break;
        };
        rest.z += step[0];
        rest.phi += step[1];
        rest.theta += step[2];
    }
    let still_draft_in = -(model.float_bottom + rest.z) / M_PER_IN;

    let wave = Wave::new(params);
    let walker = params
        .walker
        .as_ref()
        .map(|w| (w.weight_lb * KG_PER_LB * G, w));
    let load_at = |t: f64| {
        walker.map(|(weight, w)| {
            let p = walker_at(w, t);
            (weight, [in_m(p[0] - cg[0]), in_m(p[1] - cg[1])])
        })
    };

    let steps = (params.duration_s / DT).round() as usize;
    let every = ((1.0 / FPS) / DT).round().max(1.0) as usize;
    let mut s = rest;
    let mut frames = Vec::with_capacity(steps / every + 1);
    let mut summary = Summary {
        still_draft_in,
        static_heel_deg: rest.theta.to_degrees(),
        static_trim_deg: -rest.phi.to_degrees(),
        heave_period_s: 2.0 * std::f64::consts::PI * (model.m_eff / k_heave).sqrt(),
        min_freeboard_in: f64::INFINITY,
        wave_length_in: if wave.k > 0.0 {
            2.0 * std::f64::consts::PI / wave.k / M_PER_IN
        } else {
            0.0
        },
        ..Summary::default()
    };
    for i in 0..=steps {
        let t = i as f64 * DT;
        let load = load_at(t);
        let d = model.derivative(&s, &wave, t, load);
        // Deck motion and clearances at this instant.
        let mut freeboard = f64::INFINITY;
        for c in &model.corners {
            let deck = model.deck_rel + Model::dz(&s, c[0], c[1]);
            let water = wave.eta(model.cg_plan[0] + c[0], model.cg_plan[1] + c[1], t);
            freeboard = freeboard.min((deck - water) / M_PER_IN);
            let accel = d.vz + d.vphi * c[1] - d.vtheta * c[0];
            summary.max_deck_accel_g = summary.max_deck_accel_g.max(accel.abs() / G);
        }
        for &(x, y, _, _) in &model.floats {
            let bottom = model.float_bottom + Model::dz(&s, x, y);
            let surface = wave.eta(model.cg_plan[0] + x, model.cg_plan[1] + y, t);
            summary.max_float_emergence_in = summary
                .max_float_emergence_in
                .max((bottom - surface) / M_PER_IN);
        }
        summary.min_freeboard_in = summary.min_freeboard_in.min(freeboard);
        let heave = (s.z - rest.z) / M_PER_IN;
        summary.max_heave_in = summary.max_heave_in.max(heave.abs());
        summary.max_roll_deg = summary
            .max_roll_deg
            .max((s.theta - rest.theta).to_degrees().abs());
        summary.max_pitch_deg = summary
            .max_pitch_deg
            .max((s.phi - rest.phi).to_degrees().abs());
        if i % every == 0 {
            frames.push(Frame {
                t,
                heave_in: heave,
                roll_deg: s.theta.to_degrees(),
                pitch_deg: -s.phi.to_degrees(),
                min_freeboard_in: freeboard,
                walker: params.walker.as_ref().map(|w| walker_at(w, t)),
            });
        }
        if i == steps {
            break;
        }
        // Classical RK4.
        let k1 = d;
        let k2 = model.derivative(
            &add(&s, &k1, DT * 0.5),
            &wave,
            t + DT * 0.5,
            load_at(t + DT * 0.5),
        );
        let k3 = model.derivative(
            &add(&s, &k2, DT * 0.5),
            &wave,
            t + DT * 0.5,
            load_at(t + DT * 0.5),
        );
        let k4 = model.derivative(&add(&s, &k3, DT), &wave, t + DT, load_at(t + DT));
        s = State {
            z: s.z + DT / 6.0 * (k1.z + 2.0 * k2.z + 2.0 * k3.z + k4.z),
            phi: s.phi + DT / 6.0 * (k1.phi + 2.0 * k2.phi + 2.0 * k3.phi + k4.phi),
            theta: s.theta + DT / 6.0 * (k1.theta + 2.0 * k2.theta + 2.0 * k3.theta + k4.theta),
            vz: s.vz + DT / 6.0 * (k1.vz + 2.0 * k2.vz + 2.0 * k3.vz + k4.vz),
            vphi: s.vphi + DT / 6.0 * (k1.vphi + 2.0 * k2.vphi + 2.0 * k3.vphi + k4.vphi),
            vtheta: s.vtheta
                + DT / 6.0 * (k1.vtheta + 2.0 * k2.vtheta + 2.0 * k3.vtheta + k4.vtheta),
        };
        if !(s.z.is_finite() && s.phi.is_finite() && s.theta.is_finite()) {
            return Err("simulation diverged; the dock is unstable in these waves".into());
        }
    }

    let mut diagnostics = Vec::new();
    let tag = |sev, code: &str, msg: String| Diagnostic::new(sev, "simulation", code, msg, vec![]);
    let r1 = |v: f64| (v * 10.0).round() / 10.0;
    if summary.min_freeboard_in < 0.0 {
        diagnostics.push(tag(
            Severity::Error,
            "SIM-01",
            format!(
                "green water: a deck corner goes {} in under the surface",
                r1(-summary.min_freeboard_in)
            ),
        ));
    }
    if summary.max_float_emergence_in > 0.0 {
        diagnostics.push(tag(
            Severity::Warning,
            "SIM-02",
            format!(
                "floats lift clear of the water by up to {} in; expect slamming",
                r1(summary.max_float_emergence_in)
            ),
        ));
    }
    if summary.max_deck_accel_g > 0.2 {
        diagnostics.push(tag(
            Severity::Warning,
            "SIM-03",
            format!(
                "deck corners accelerate up to {} g; over 0.2 g is hard to stand on",
                (summary.max_deck_accel_g * 100.0).round() / 100.0
            ),
        ));
    }
    Ok(SimResult {
        params: Some(params.clone()),
        fps: 1.0 / (every as f64 * DT),
        pivot: cg,
        water_level_in: still_draft_in,
        frames,
        summary,
        diagnostics,
        coefficients: Some(coef),
    })
}

fn solve3(a: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let det = |m: &[[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let d = det(&a);
    if d.abs() < 1e-30 {
        return None;
    }
    let mut out = [0.0; 3];
    for (k, slot) in out.iter_mut().enumerate() {
        let mut m = a;
        for i in 0..3 {
            m[i][k] = b[i];
        }
        *slot = det(&m) / d;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use liquidcad_core::geometry::{MassIntegrals, Polyhedron, Pose};

    /// A 4 x 8 grid of 24 in floats carrying a uniform 96 x 192 x 20 in
    /// block of `mass` lb.
    fn barge(mass: f64) -> Body {
        let mut floats = Vec::new();
        for r in 0..8 {
            for c in 0..4 {
                floats.push(FloatBox {
                    min: [c as f64 * 24.0, r as f64 * 24.0, 0.0],
                    size: [24.0, 24.0, 16.0],
                });
            }
        }
        let block: MassIntegrals = Polyhedron::cuboid([96.0, 192.0, 20.0]).integrals();
        let density = mass / block.mass;
        Body {
            mass: MassProps::uniform(&block, density, &Pose::IDENTITY),
            floats,
            deck_top_in: 20.0,
            deck_corners: [[0.0, 0.0], [96.0, 0.0], [96.0, 192.0], [0.0, 192.0]],
        }
    }

    fn calm(duration: f64) -> SimParams {
        SimParams {
            wave_height_in: 0.0,
            period_s: 4.0,
            heading_deg: 0.0,
            duration_s: duration,
            walker: None,
        }
    }

    #[test]
    fn rests_at_the_hydrostatic_draft() {
        let r = simulate(&barge(1500.0), &calm(5.0)).unwrap();
        // 1500 lb on 96 x 192 in of waterplane at 62.4 lb/ft³.
        let expected = 1500.0 / (62.4 * (96.0 * 192.0 / 144.0)) * 12.0;
        assert!(
            (r.summary.still_draft_in - expected).abs() < 0.01,
            "{}",
            r.summary.still_draft_in
        );
        assert!(r.summary.max_heave_in < 1e-6, "{}", r.summary.max_heave_in);
        assert!(r.summary.static_heel_deg.abs() < 1e-6);
        assert!(r.diagnostics.is_empty());
        // Weight toward the stern trims the stern down (negative trim) and
        // toward starboard heels starboard down (positive heel).
        let mut body = barge(1500.0);
        let shift = Pose::translation([6.0, -20.0, 0.0]);
        body.mass = body.mass.transformed(&shift);
        let r = simulate(&body, &calm(2.0)).unwrap();
        assert!(
            r.summary.static_trim_deg < 0.0 && r.summary.static_heel_deg > 0.0,
            "{:?}",
            r.summary
        );
    }

    #[test]
    fn heave_oscillates_at_the_natural_period() {
        let body = barge(1500.0);
        let coef = Coefficients {
            damping_ratio: 0.0,
            ..Coefficients::default()
        };
        // A 10 lb drop of the walker starts a free heave oscillation.
        let p = SimParams {
            walker: Some(Walker {
                weight_lb: 150.0,
                from: [48.0, 96.0],
                to: [48.0, 96.0],
                speed_in_s: 10.0,
            }),
            ..calm(6.0)
        };
        let r = simulate_with(&body, &p, coef).unwrap();
        // Zero crossings of heave about its new mean give the period.
        let mean = r.frames.iter().map(|f| f.heave_in).sum::<f64>() / r.frames.len() as f64;
        let ups: Vec<f64> = r
            .frames
            .windows(2)
            .filter(|w| w[0].heave_in < mean && w[1].heave_in >= mean)
            .map(|w| w[1].t)
            .collect();
        assert!(ups.len() >= 3, "{ups:?}");
        let period = (ups[ups.len() - 1] - ups[0]) / (ups.len() - 1) as f64;
        let m_eff = 1500.0 * KG_PER_LB * 1.5;
        let k = RHO * G * (96.0 * 192.0) * M_PER_IN * M_PER_IN;
        let expected = 2.0 * std::f64::consts::PI * (m_eff / k).sqrt();
        assert!((period - expected).abs() < 0.05, "{period} vs {expected}");
        assert!((r.summary.heave_period_s - expected).abs() < 1e-9);
    }

    #[test]
    fn long_beam_waves_are_followed_and_short_ones_slam() {
        // λ = gT²/2π ≈ 553 in for 3 s, far wider than the 96 in beam: the
        // dock rides the surface, rolling with the wave slope kA.
        let long = SimParams {
            wave_height_in: 36.0,
            period_s: 3.0,
            heading_deg: 90.0,
            duration_s: 20.0,
            walker: None,
        };
        let r = simulate(&barge(1500.0), &long).unwrap();
        let slope = (2.0 * std::f64::consts::PI / r.summary.wave_length_in * 18.0)
            .atan()
            .to_degrees();
        assert!(
            (r.summary.max_roll_deg - slope).abs() / slope < 0.15,
            "{:?}",
            r.summary
        );
        assert!(
            (r.summary.max_heave_in - 18.0).abs() < 2.0,
            "{:?}",
            r.summary
        );
        assert!(r.summary.max_pitch_deg < 1e-9, "beam seas do not pitch");
        assert!(r.diagnostics.iter().any(|d| d.code == "SIM-03"));
        assert_eq!(r.frames.len(), 601);
        // λ ≈ 138 in for 1.5 s: crest and trough sit under opposite sides,
        // so floats come out of the water.
        let short = SimParams {
            period_s: 1.5,
            wave_height_in: 24.0,
            ..long.clone()
        };
        let r = simulate(&barge(1500.0), &short).unwrap();
        assert!(
            r.diagnostics.iter().any(|d| d.code == "SIM-02"),
            "{:?}",
            r.summary
        );
        // Head seas pitch rather than roll.
        let head = simulate(
            &barge(1500.0),
            &SimParams {
                heading_deg: 0.0,
                ..long
            },
        )
        .unwrap();
        assert!(head.summary.max_pitch_deg > 0.5 && head.summary.max_roll_deg < 1e-9);
    }

    #[test]
    fn inputs_are_bounded() {
        let mut p = calm(10.0);
        p.duration_s = 1000.0;
        assert!(simulate(&barge(1500.0), &p).is_err());
        p.duration_s = 10.0;
        p.period_s = f64::NAN;
        assert!(simulate(&barge(1500.0), &p).is_err());
    }
}
