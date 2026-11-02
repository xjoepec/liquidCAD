use liquidcad_core::{rebuild, templates::modular_dock, Project};

#[test]
fn json_roundtrip_preserves_the_model() {
    let project = modular_dock();
    let json = project.to_json().unwrap();
    let loaded = Project::from_json(&json).unwrap();
    assert_eq!(loaded.parameters["width_in"].value, 96.0);
    assert_eq!(loaded.features.len(), project.features.len());
    let a = rebuild(&project, 1).unwrap();
    let b = rebuild(&loaded, 1).unwrap();
    assert_eq!(a.components.len(), b.components.len());
    assert_eq!(
        serde_json::to_string(&a.components).unwrap(),
        serde_json::to_string(&b.components).unwrap()
    );
}

#[test]
fn bundled_sample_matches_the_template() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../projects/modular-dock/liquidcad.json");
    let bundled = Project::load_path(&path).expect("bundled sample loads");
    assert_eq!(
        bundled.to_json().unwrap(),
        modular_dock().to_json().unwrap(),
        "regenerate with: cargo run -p liquidcad-cli -- new-dock projects/modular-dock/liquidcad.json"
    );
}

#[test]
fn rejects_malformed_projects() {
    assert!(Project::from_json("{}").is_err());
    let mut value: serde_json::Value =
        serde_json::from_str(&modular_dock().to_json().unwrap()).unwrap();
    value["schema"] = 99.into();
    assert!(Project::from_json(&value.to_string()).is_err());
}
