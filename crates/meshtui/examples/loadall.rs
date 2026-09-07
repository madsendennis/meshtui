//! Load-only smoke test: `cargo run --example loadall -- <paths...>`.
//! Prints one line per file with OK/err — used to validate loaders against
//! large real-world datasets without rendering.

use std::path::Path;

use meshtui_core::loaders::load_meshes;

fn main() {
    let mut fails = 0;
    let mut total = 0;
    for arg in std::env::args().skip(1) {
        total += 1;
        match load_meshes(Path::new(&arg)) {
            Ok(meshes) => {
                let tris: usize = meshes.iter().map(|m| m.triangle_count()).sum();
                println!("OK {arg} ({} meshes, {tris} tris)", meshes.len());
            }
            Err(e) => {
                fails += 1;
                println!("FAIL {arg}: {e}");
            }
        }
    }
    eprintln!("total={total} fails={fails}");
    std::process::exit(if fails > 0 { 1 } else { 0 });
}
