//! Finds OpenCascade and, when present, compiles the C++ adapter against it.
//! Without OCCT the crate uses its Rust B-rep kernel and needs no C++ at all.
//!
//! Search order: OCCT_ROOT, <workspace>/.occt (conda prefix), Homebrew,
//! /usr/local, /usr, /opt/opencascade. Set LIQUIDCAD_NO_OCCT=1 to skip.

use std::env;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rustc-check-cfg=cfg(occt)");
    println!("cargo:rerun-if-changed=cpp/occt_adapter.cpp");
    println!("cargo:rerun-if-changed=cpp/occt_adapter.h");
    println!("cargo:rerun-if-env-changed=OCCT_ROOT");
    println!("cargo:rerun-if-env-changed=LIQUIDCAD_NO_OCCT");

    if env::var_os("LIQUIDCAD_NO_OCCT").is_some() {
        println!("cargo:warning=LIQUIDCAD_NO_OCCT set; using the Rust B-rep kernel");
        return;
    }
    let Some(found) = find_occt() else {
        println!(
            "cargo:warning=OpenCascade not found; using the Rust B-rep kernel. Set OCCT_ROOT to link OCCT."
        );
        return;
    };

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .file("cpp/occt_adapter.cpp")
        .include("cpp")
        .include(&found.include)
        .flag_if_supported("-std=c++17")
        .flag_if_supported("/std:c++17")
        .flag_if_supported("/EHsc")
        .warnings(false);
    build.compile("liquidcad_occt_adapter");

    println!("cargo:rustc-link-search=native={}", found.lib.display());
    let modern = lib_exists(&found.lib, "TKDESTEP");
    let mut libs = vec![
        "TKernel",
        "TKMath",
        "TKG2d",
        "TKG3d",
        "TKGeomBase",
        "TKBRep",
        "TKGeomAlgo",
        "TKTopAlgo",
        "TKPrim",
        "TKMesh",
        "TKShHealing",
        "TKXSBase",
        // XCAF document: names and colours in STEP.
        "TKCDF",
        "TKLCAF",
        "TKCAF",
        "TKXCAF",
        "TKService",
        "TKV3d",
        "TKVCAF",
        // Fillet, chamfer, booleans.
        "TKFillet",
        "TKBO",
        "TKBool",
    ];
    if modern {
        // OCCT 7.8 folded the STEP toolkits into TKDESTEP.
        libs.push("TKDE");
        libs.push("TKDESTEP");
    } else {
        libs.extend([
            "TKSTEPBase",
            "TKSTEPAttr",
            "TKSTEP209",
            "TKSTEP",
            "TKXDESTEP",
        ]);
    }
    for lib in libs {
        if matches!(
            lib,
            "TKDE" | "TKDECascade" | "TKVCAF" | "TKService" | "TKV3d"
        ) && !lib_exists(&found.lib, lib)
        {
            continue;
        }
        println!("cargo:rustc-link-lib=dylib={lib}");
    }
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "macos" {
        println!("cargo:rustc-link-lib=c++");
    } else if target_os == "linux" {
        println!("cargo:rustc-link-lib=stdc++");
    }
    if target_os != "windows" {
        // Our own tests need the rpath; dependents read DEP_LIQUIDCAD_OCCT_LIB_DIR.
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", found.lib.display());
    }
    println!("cargo:lib_dir={}", found.lib.display());
    println!("cargo:rustc-cfg=occt");
    println!(
        "cargo:warning=linking OpenCascade from {} ({})",
        found.root.display(),
        if modern { "TKDESTEP" } else { "TKSTEP" }
    );
}

struct Occt {
    root: PathBuf,
    include: PathBuf,
    lib: PathBuf,
}

fn find_occt() -> Option<Occt> {
    let workspace = PathBuf::from(env::var("CARGO_MANIFEST_DIR").ok()?).join("../..");
    let mut roots = Vec::new();
    if let Ok(root) = env::var("OCCT_ROOT") {
        roots.push(PathBuf::from(root));
    }
    roots.push(workspace.join(".occt"));
    // Re-run when a conda prefix appears or is replaced.
    println!(
        "cargo:rerun-if-changed={}",
        workspace.join(".occt/conda-meta").display()
    );
    roots.extend(
        [
            "/opt/homebrew/opt/opencascade",
            "/usr/local/opt/opencascade",
            "/usr/local",
            "/usr",
            "/opt/opencascade",
        ]
        .map(PathBuf::from),
    );
    for root in roots {
        let include = [
            "include/opencascade",
            "Library/include/opencascade",
            "inc",
            "include",
        ]
        .iter()
        .map(|p| root.join(p))
        .find(|p| p.join("Standard.hxx").exists());
        let lib = [
            "lib",
            "lib64",
            "lib/x86_64-linux-gnu",
            "lib/aarch64-linux-gnu",
            "Library/lib",
            "win64/vc14/lib",
        ]
        .iter()
        .map(|p| root.join(p))
        .find(|p| lib_exists(p, "TKernel"));
        if let (Some(include), Some(lib)) = (include, lib) {
            return Some(Occt { root, include, lib });
        }
    }
    None
}

fn lib_exists(dir: &Path, name: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let file = entry.file_name();
        let file = file.to_string_lossy();
        file == format!("lib{name}.dylib")
            || file == format!("lib{name}.so")
            || file.starts_with(&format!("lib{name}.so."))
            || file == format!("{name}.lib")
    })
}
