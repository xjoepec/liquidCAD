//! `liquidcad`: headless CLI and the JSON-lines engine server used by the
//! desktop app.

use liquidcad_core::templates::modular_dock;
use liquidcad_core::{ComponentKind, Severity};
use liquidcad_engine::{default_project_path, Engine, EngineConfig, EngineError};
use liquidcad_protocol::{Envelope, Event, ExportFormat, Response, Value};
use std::collections::VecDeque;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first() else {
        print_help();
        return ExitCode::SUCCESS;
    };
    let rest = &args[1..];
    let result = match command.as_str() {
        "help" | "-h" | "--help" => {
            print_help();
            Ok(())
        }
        "version" | "--version" => {
            let status = Engine::status();
            println!("liquidcad {}  kernel: {}", status.version, status.kernel);
            Ok(())
        }
        "serve" => return serve(rest),
        "new-dock" => new_dock(rest),
        "info" => info(rest),
        "set" => set(rest),
        "export" => export(rest),
        "verify" => verify(rest),
        "eval" => eval(rest),
        other => Err(format!("unknown command {other}; run liquidcad help")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    eprintln!(
        "liquidCAD engine {}

USAGE
  liquidcad serve [--data-dir DIR]          JSON-lines engine for the desktop app
  liquidcad new-dock [PATH]                 write the modular dock sample project
  liquidcad info [PROJECT]                  summary, diagnostics, BOM
  liquidcad set PROJECT NAME VALUE [--save] validate and apply a parameter
  liquidcad export [--project P] [--out DIR] [--formats dxf,step,pdf,csv]
  liquidcad verify DIR                      re-read and check exported artifacts
  liquidcad eval [PROJECT] [OUT]            run the first-milestone script headless

PROJECT defaults to projects/modular-dock/liquidcad.json.",
        env!("CARGO_PKG_VERSION")
    );
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn positional(args: &[String]) -> Vec<&str> {
    let mut out = Vec::new();
    let mut skip = false;
    for arg in args {
        if skip {
            skip = false;
            continue;
        }
        if arg.starts_with("--") {
            skip = !matches!(arg.as_str(), "--save");
            continue;
        }
        out.push(arg.as_str());
    }
    out
}

fn open(path: Option<&str>) -> Result<Engine, String> {
    let path = path.map(PathBuf::from).unwrap_or_else(default_project_path);
    let data_dir = std::env::var_os("LIQUIDCAD_DATA_DIR").map(PathBuf::from);
    let mut engine = Engine::new(EngineConfig { data_dir }).map_err(|e| e.to_string())?;
    engine
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(engine)
}

fn err(e: EngineError) -> String {
    e.to_string()
}

// ---------------------------------------------------------------- serve

/// Reads requests on a dedicated thread and runs them on this one, so the
/// pipe never backs up while a rebuild runs. Back-to-back edits of the same
/// parameter are coalesced: the earlier one is answered `superseded`.
/// The protocol gets stdout to itself. On Unix the original stdout is moved to
/// a private descriptor and fd 1 is pointed at stderr, so anything a native
/// library prints (OpenCascade reports progress on std::cout) lands in the
/// engine log instead of corrupting the JSON-lines stream.
#[cfg(unix)]
fn protocol_channel() -> Box<dyn Write> {
    use std::os::fd::FromRawFd;
    // SAFETY: dup/dup2 on the process's own standard descriptors. The
    // duplicated fd is owned by the returned File and closed once.
    unsafe {
        let fd = libc::dup(libc::STDOUT_FILENO);
        if fd < 0 || libc::dup2(libc::STDERR_FILENO, libc::STDOUT_FILENO) < 0 {
            return Box::new(std::io::stdout());
        }
        Box::new(std::io::BufWriter::new(std::fs::File::from_raw_fd(fd)))
    }
}

#[cfg(not(unix))]
fn protocol_channel() -> Box<dyn Write> {
    Box::new(std::io::stdout())
}

fn serve(args: &[String]) -> ExitCode {
    let data_dir = flag(args, "--data-dir").map(PathBuf::from);
    let mut engine = match Engine::new(EngineConfig { data_dir }) {
        Ok(engine) => engine,
        Err(e) => {
            eprintln!("engine failed to start: {e}");
            return ExitCode::FAILURE;
        }
    };
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            match line {
                Ok(line) if line.trim().is_empty() => continue,
                Ok(line) => {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut out = protocol_channel();
    let mut emit =
        |text: String| -> bool { writeln!(out, "{text}").is_ok() && out.flush().is_ok() };
    if !emit(serde_json::to_string(&Event::Ready(Engine::status())).unwrap_or_default()) {
        return ExitCode::FAILURE;
    }

    let mut queue: VecDeque<Result<Envelope, (u64, String)>> = VecDeque::new();
    loop {
        if queue.is_empty() {
            match rx.recv() {
                Ok(line) => queue.push_back(parse(&line)),
                Err(_) => break,
            }
        }
        while let Ok(line) = rx.try_recv() {
            queue.push_back(parse(&line));
        }
        let Some(item) = queue.pop_front() else {
            continue;
        };
        let response = match item {
            Err((id, message)) => Response::err(id, "protocol", message),
            Ok(envelope) => {
                let superseded = envelope.payload.coalesce_key().is_some_and(|key| {
                    matches!(queue.front(), Some(Ok(next)) if next.payload.coalesce_key().as_deref() == Some(key.as_str()))
                });
                if superseded {
                    Response::ok(envelope.id, Value::Superseded)
                } else {
                    engine.handle(envelope.id, envelope.payload)
                }
            }
        };
        let text = serde_json::to_string(&response).unwrap_or_else(|e| {
            serde_json::to_string(&Response::err(response.id, "internal", e.to_string()))
                .unwrap_or_default()
        });
        if !emit(text) {
            break;
        }
    }
    ExitCode::SUCCESS
}

fn parse(line: &str) -> Result<Envelope, (u64, String)> {
    serde_json::from_str::<Envelope>(line).map_err(|e| {
        let id = serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .and_then(|v| v.get("id").and_then(|id| id.as_u64()))
            .unwrap_or(0);
        (id, e.to_string())
    })
}

// ------------------------------------------------------------- commands

fn new_dock(args: &[String]) -> Result<(), String> {
    let path = args
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(default_project_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = modular_dock().to_json().map_err(|e| e.to_string())?;
    std::fs::write(&path, json + "\n").map_err(|e| e.to_string())?;
    println!("wrote {}", path.display());
    Ok(())
}

fn info(args: &[String]) -> Result<(), String> {
    let engine = open(args.first().map(String::as_str))?;
    let doc = engine.document().map_err(err)?;
    let e = &doc.engineering;
    println!("{}  ({})", doc.project.name, Engine::status().kernel);
    for p in doc.project.parameters.values() {
        println!("  {:<20} {:>8} {}", p.name, p.value, p.unit.suffix());
    }
    println!(
        "{} components, {:.0} lb dead, {:.0} lb displacement, {:.0}% reserve, freeboard {:.1} in",
        doc.components.len(),
        e.dead_load_lb,
        e.buoyancy_capacity_lb,
        e.reserve_buoyancy_ratio * 100.0,
        e.freeboard_dead_in
    );
    for d in &doc.diagnostics {
        println!("  [{:?}] {} {}", d.severity, d.code, d.message);
    }
    for line in &doc.bom {
        println!(
            "  {:>3} x {:<14} {:<34} {:>8.1} lb",
            line.qty, line.item, line.size, line.total_weight_lb
        );
    }
    Ok(())
}

fn set(args: &[String]) -> Result<(), String> {
    let pos = positional(args);
    let [project, name, value, ..] = pos.as_slice() else {
        return Err("usage: liquidcad set PROJECT NAME VALUE [--save]".into());
    };
    let value: f64 = value
        .parse()
        .map_err(|_| format!("{value} is not a number"))?;
    let mut engine = open(Some(project))?;
    engine.set_parameter(name, value).map_err(err)?;
    let report = engine.last_report();
    println!(
        "{name} = {value}; regenerated {:?}, reused {}",
        report
            .regenerated
            .iter()
            .map(|f| f.as_str())
            .collect::<Vec<_>>(),
        report.reused.len()
    );
    if args.iter().any(|a| a == "--save") {
        let path = engine.save(None).map_err(err)?;
        println!("saved {}", path.display());
    }
    Ok(())
}

fn formats(args: &[String]) -> Result<Vec<ExportFormat>, String> {
    match flag(args, "--formats") {
        None => Ok(ExportFormat::ALL.to_vec()),
        Some(list) => list
            .split(',')
            .map(|f| {
                serde_json::from_value(serde_json::Value::String(f.trim().to_lowercase()))
                    .map_err(|_| format!("unknown format {f}"))
            })
            .collect(),
    }
}

fn export_all(engine: &mut Engine, out: &Path, formats: &[ExportFormat]) -> Result<(), String> {
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    for format in formats {
        let path = out.join(format!("dock.{}", format.extension()));
        let bytes = engine.export(*format, &path).map_err(err)?;
        println!(
            "  {:<5} {:>9} bytes  {}",
            format.extension(),
            bytes,
            path.display()
        );
    }
    Ok(())
}

fn export(args: &[String]) -> Result<(), String> {
    let mut engine = open(flag(args, "--project"))?;
    let out = PathBuf::from(flag(args, "--out").unwrap_or("artifacts"));
    export_all(&mut engine, &out, &formats(args)?)
}

/// Re-reads exported files with the format checkers.
fn verify(args: &[String]) -> Result<(), String> {
    let dir = PathBuf::from(args.first().ok_or("usage: liquidcad verify DIR")?);
    let read = |name: &str| std::fs::read(dir.join(name)).map_err(|e| format!("{name}: {e}"));
    let text = |name: &str| {
        read(name).and_then(|b| String::from_utf8(b).map_err(|e| format!("{name}: {e}")))
    };

    let dxf =
        liquidcad_dxf::check::parse(&text("dock.dxf")?).map_err(|e| format!("dock.dxf: {e}"))?;
    println!(
        "dock.dxf   {} {} polylines ({} closed), {} lines, {} circles, {} arc vertices, {} texts, layers {:?}",
        dxf.version,
        dxf.polylines,
        dxf.closed_polylines,
        dxf.lines,
        dxf.circles,
        dxf.arc_vertices,
        dxf.texts,
        dxf.layers
    );
    let step = liquidcad_occt::step::check::parse(&text("dock.step")?)
        .map_err(|e| format!("dock.step: {e}"))?;
    let count = |kw: &str| {
        step.values()
            .filter(|i| i.keywords.iter().any(|k| k == kw))
            .count()
    };
    println!(
        "dock.step  {} instances, {} MANIFOLD_SOLID_BREP, {} ADVANCED_FACE, all references resolve",
        step.len(),
        count("MANIFOLD_SOLID_BREP"),
        count("ADVANCED_FACE")
    );
    let pdf = read("dock.pdf")?;
    if !pdf.starts_with(b"%PDF-") || !pdf.ends_with(b"%%EOF\n") {
        return Err("dock.pdf: missing PDF header or trailer".into());
    }
    println!(
        "dock.pdf   {} bytes, PDF header and trailer present",
        pdf.len()
    );
    let csv = text("dock.csv")?;
    let rows = csv.lines().count();
    if !csv.starts_with("line,item,kind") {
        return Err("dock.csv: unexpected header".into());
    }
    println!("dock.csv   {} BOM rows", rows - 1);
    Ok(())
}

/// The first working milestone, end to end, without a window.
fn eval(args: &[String]) -> Result<(), String> {
    let pos = positional(args);
    let mut engine = open(pos.first().copied())?;
    let out = PathBuf::from(pos.get(1).copied().unwrap_or("artifacts/milestone"));
    let floats = |e: &Engine| {
        e.document()
            .map(|d| {
                d.components
                    .iter()
                    .filter(|c| c.kind == ComponentKind::FloatModule)
                    .count()
            })
            .unwrap_or(0)
    };
    let step = |n: u32, text: &str| println!("{n:>2}. {text}");

    let doc = engine.document().map_err(err)?;
    step(
        3,
        &format!(
            "opened {} with {} components",
            doc.project.name,
            doc.components.len()
        ),
    );
    step(
        4,
        &format!(
            "{} display meshes from {}",
            engine.state().map_err(err)?.document.scene.meshes.len(),
            Engine::status().kernel
        ),
    );
    engine.select(Some("frame.stringer.stbd")).map_err(err)?;
    let info = engine
        .selection_info()
        .map_err(err)?
        .ok_or("selection lost")?;
    step(
        5,
        &format!(
            "selected {} ({}), driven by {:?}",
            info.component.name, info.feature_name, info.driven_by
        ),
    );
    let before_floats = floats(&engine);
    let before_x = info.component.bounds.origin[0];
    engine.set_parameter("width_in", 144.0).map_err(err)?;
    step(6, "set Dock width = 144 in");
    let report = engine.last_report().clone();
    let state = engine.state().map_err(err)?;
    step(
        7,
        &format!(
            "regenerated {:?} in {:.2} ms, reused {:?}; meshes built {} reused {}",
            report
                .regenerated
                .iter()
                .map(|f| f.as_str())
                .collect::<Vec<_>>(),
            report.elapsed_ms,
            report.reused.iter().map(|f| f.as_str()).collect::<Vec<_>>(),
            state.meshes_built,
            state.meshes_reused
        ),
    );
    let after = engine
        .selection_info()
        .map_err(err)?
        .ok_or("selection lost after rebuild")?;
    step(
        8,
        &format!(
            "{} kept its id and moved from x={} to x={}",
            after.component.id, before_x, after.component.bounds.origin[0]
        ),
    );
    let doc = engine.document().map_err(err)?;
    let bom_floats = doc
        .bom
        .iter()
        .find(|l| l.kind == "float_module")
        .map(|l| l.qty)
        .unwrap_or(0);
    if bom_floats as usize != floats(&engine) || floats(&engine) <= before_floats {
        return Err(format!(
            "BOM float count {bom_floats} does not follow geometry"
        ));
    }
    step(9, &format!("BOM floats {before_floats} -> {bom_floats}"));
    for d in doc
        .diagnostics
        .iter()
        .filter(|d| d.severity >= Severity::Warning)
    {
        println!("    [{:?}] {} {}", d.severity, d.code, d.message);
    }
    beyond_the_milestone(&mut engine)?;
    step(14, &format!("exporting to {}", out.display()));
    export_all(&mut engine, &out, &ExportFormat::ALL)?;
    verify(&[out.display().to_string()])
}

/// Sketched part, modifier, mate-driven gangway, and wave run on the
/// widened dock.
fn beyond_the_milestone(engine: &mut Engine) -> Result<(), String> {
    use liquidcad_core::feature::{ExtrudeSpec, FeatureSpec, ModifySpec, SketchPlane};
    use liquidcad_core::geometry::{PathVertex, Profile, ShapeOp};
    use liquidcad_protocol::FeatureDraft;
    let step = |n: u32, text: &str| println!("{n:>2}. {text}");
    let mass = |e: &Engine, id: &str| -> Result<f64, String> {
        Ok(e.document()
            .map_err(err)?
            .component(&liquidcad_core::ComponentId::new(id))
            .map_err(|e| e.to_string())?
            .mass_lb())
    };

    let bracket = FeatureDraft {
        name: "Ladder bracket".into(),
        material: Some("alum_6061".into()),
        spec: FeatureSpec::Extrude(ExtrudeSpec {
            plane: SketchPlane::Face {
                component: "deck.board.6".into(),
                face: "z+".into(),
            },
            profile: Profile {
                outer: [[0., 0.], [6., 0.], [6., 2.], [2., 2.], [2., 6.], [0., 6.]]
                    .iter()
                    .map(|p| PathVertex::line(p[0], p[1]))
                    .collect(),
                holes: vec![],
            },
            depth: 0.25,
            depth_parameter: None,
            direction: Default::default(),
            cut_into: None,
        }),
    };
    let id = engine.add_feature(bracket).map_err(err)?;
    let custom = format!("custom.{}", id.as_str().trim_start_matches("feat."));
    step(
        10,
        &format!(
            "sketched an L bracket on deck.board.6 z+ and extruded {}: {:.3} lb",
            custom,
            mass(engine, &custom)?
        ),
    );

    let before = mass(engine, "frame.stringer.port")?;
    let hole = FeatureDraft {
        name: "Bolt hole".into(),
        material: None,
        spec: FeatureSpec::Modify(ModifySpec {
            target: "frame.stringer.port".into(),
            op: ShapeOp::Hole {
                face: "side.3".into(),
                at: [0.0, 2.0, 24.0],
                diameter: 0.5,
                depth: 0.0,
            },
        }),
    };
    match engine.add_feature(hole) {
        Ok(_) => step(
            11,
            &format!(
                "drilled a 1/2 in bolt hole through frame.stringer.port: {:.4} lb removed from its two 1/8 in walls",
                before - mass(engine, "frame.stringer.port")?
            ),
        ),
        Err(e) => step(11, &format!("bolt hole refused: {e}")),
    }

    engine
        .set_parameter("gangway_angle_deg", 10.0)
        .map_err(err)?;
    let state = engine.state().map_err(err)?;
    let gang = state
        .document
        .diagnostics
        .iter()
        .any(|d| d.code == "GANG-01");
    step(
        12,
        &format!(
            "gangway slope 10 deg: hinge mate re-solved, meshes built {} reused {}, GANG-01 {}",
            state.meshes_built,
            state.meshes_reused,
            if gang { "raised" } else { "clear" }
        ),
    );

    let sim = engine
        .simulate(&liquidcad_sim::SimParams {
            wave_height_in: 12.0,
            period_s: 2.5,
            heading_deg: 90.0,
            duration_s: 10.0,
            walker: None,
        })
        .map_err(err)?;
    let s = &sim.summary;
    step(
        13,
        &format!(
            "10 s of 12 in beam waves: heave {:.1} in, heel {:.1} deg, trim {:.1} deg, min freeboard {:.1} in, {} frames at {} fps{}",
            s.max_heave_in,
            s.max_roll_deg,
            s.max_pitch_deg,
            s.min_freeboard_in,
            sim.frames.len(),
            sim.fps,
            sim.diagnostics
                .iter()
                .map(|d| format!("; {} {}", d.code, d.message))
                .collect::<String>()
        ),
    );
    Ok(())
}
