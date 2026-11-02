// Adds the OpenCascade library directory to the rpath of this crate's
// binaries (and the liquidcad executable) when liquidcad-occt linked OCCT dynamically.
fn main() {
    println!("cargo:rerun-if-env-changed=DEP_LIQUIDCAD_OCCT_LIB_DIR");
    if let Ok(dir) = std::env::var("DEP_LIQUIDCAD_OCCT_LIB_DIR") {
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");
        }
    }
}
