//! Loads every file in `testdata/meshes/` and asserts it parses to non-empty
//! geometry. Regenerate the data with `python3 testdata/generate.py` and
//! `cargo run -p meshtui-core --example gen_testdata_draco`.

use std::path::Path;

use meshtui_core::loaders::load_meshes;

#[test]
fn all_testdata_meshes_load() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/meshes");
    assert!(dir.is_dir(), "missing {}", dir.display());

    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        // .mtl/.png are referenced by cube_textured.obj, not meshes themselves.
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !matches!(
            ext.to_ascii_lowercase().as_str(),
            "stl" | "obj" | "ply" | "drc" | "glb"
        ) {
            continue;
        }
        let meshes =
            load_meshes(&path).unwrap_or_else(|e| panic!("{} failed to load: {e}", path.display()));
        assert!(
            meshes.iter().any(|m| m.triangle_count() > 0),
            "{} loaded with no triangles",
            path.display()
        );
        checked += 1;
    }
    assert!(
        checked >= 14,
        "expected at least 14 test meshes, found {checked}"
    );
}
