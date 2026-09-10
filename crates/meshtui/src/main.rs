//! meshtui — terminal 3D mesh viewer (Rust rewrite).
//!
//! Usage:
//!   meshtui <mesh> [more meshes...]   launch the TUI
//!   meshtui <mesh> --screenshot out.png [--size WxH]   headless render
//!   meshtui --config user.toml ...
//!
//! Exit codes: 0 success, 1 error (unlike the Python version, where Typer
//! swallowed the return value and every failure exited 0).

mod app;
mod commands;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use meshtui_core::config::{self, Config};
use meshtui_core::loaders::load_meshes;
use meshtui_core::Scene;

fn usage() -> &'static str {
    "meshtui — terminal 3D mesh viewer\n\
     \n\
     Usage:\n\
     \x20 meshtui <mesh> [more meshes...]              launch the TUI\n\
     \x20 meshtui <mesh...> --screenshot out.png       headless render to PNG\n\
     \n\
     Supported formats: .ply, .stl, .obj, .drc, .glb\n\
     \n\
     Options:\n\
     \x20 --config <path>     extra TOML config merged over the user config\n\
     \x20 --size <WxH>        screenshot size (default 1600x1200)\n\
     \x20 --print-config      print the merged effective config and exit\n\
     \x20 --write-default-config[=<path>]\n\
     \x20                     write all defaults to the user config file\n\
     \x20                     (default: ~/.config/meshtui/config.toml)\n\
     \x20 --version           print the version and exit\n\
     \x20 --help              this message\n\
     \n\
     The user config is created with all defaults on first run and loaded\n\
     on every run. Precedence: embedded defaults < user config < --config."
}

struct Args {
    meshes: Vec<PathBuf>,
    config: Option<PathBuf>,
    screenshot: Option<PathBuf>,
    size: (u32, u32),
    print_config: bool,
    write_default_config: Option<Option<PathBuf>>,
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut args = Args {
        meshes: Vec::new(),
        config: None,
        screenshot: None,
        size: (1600, 1200),
        print_config: false,
        write_default_config: None,
    };
    let mut it = argv.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--help" | "-h" => return Err(usage().to_string()),
            "--config" => {
                args.config = Some(PathBuf::from(it.next().ok_or("--config needs a path")?))
            }
            "--print-config" => args.print_config = true,
            "--write-default-config" => args.write_default_config = Some(None),
            other if other.starts_with("--write-default-config=") => {
                let path = &other["--write-default-config=".len()..];
                if path.is_empty() {
                    return Err("--write-default-config=<path> needs a non-empty path".into());
                }
                args.write_default_config = Some(Some(PathBuf::from(path)));
            }
            "--screenshot" => {
                args.screenshot = Some(PathBuf::from(it.next().ok_or("--screenshot needs a path")?))
            }
            "--size" => {
                let s = it.next().ok_or("--size needs WxH")?;
                let (w, h) = s.split_once('x').ok_or("--size format: WxH")?;
                args.size = (
                    w.parse().map_err(|_| "bad width")?,
                    h.parse().map_err(|_| "bad height")?,
                );
                if args.size.0 == 0 || args.size.1 == 0 || args.size.0 > 4096 || args.size.1 > 4096
                {
                    return Err("--size dimensions must be between 1 and 4096".into());
                }
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other => args.meshes.push(PathBuf::from(other)),
        }
    }
    if args.meshes.is_empty() && !args.print_config && args.write_default_config.is_none() {
        return Err(usage().to_string());
    }
    Ok(args)
}

fn run() -> Result<(), String> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = parse_args(&argv)?;

    // Config-only commands run without meshes.
    if let Some(target) = &args.write_default_config {
        let path = match target {
            Some(p) => p.clone(),
            None => config::user_config_path()
                .ok_or("could not determine user config directory (is HOME set?)")?,
        };
        config::write_default_config(&path).map_err(|e| e.to_string())?;
        println!("wrote default config to {}", path.display());
        return Ok(());
    }
    if args.print_config {
        let text =
            config::effective_config_toml(args.config.as_deref()).map_err(|e| e.to_string())?;
        println!("{text}");
        return Ok(());
    }

    // First run: place a fully commented copy of the defaults at the XDG
    // config path. Best-effort — a read-only home must not block startup.
    match config::ensure_user_config() {
        Ok(Some(path)) => eprintln!("meshtui: wrote default config to {}", path.display()),
        Ok(None) => {}
        Err(e) => eprintln!("meshtui: warning: could not write user config: {e}"),
    }

    let config = Config::load_effective(args.config.as_deref()).map_err(|e| e.to_string())?;

    let mut scene = Scene::new();
    let mut paths: Vec<PathBuf> = Vec::new();
    for path in &args.meshes {
        if path.is_dir() {
            // Directory: load every supported mesh file inside (sorted,
            // non-recursive — matches the Python mesh explorer).
            let mut entries = Vec::new();
            for entry in std::fs::read_dir(path).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let path = entry.path();
                if path.is_file()
                    && matches!(
                        path.extension()
                            .and_then(|e| e.to_str())
                            .map(str::to_ascii_lowercase)
                            .as_deref(),
                        Some("stl" | "obj" | "ply" | "drc" | "glb")
                    )
                {
                    entries.push(path);
                }
            }
            entries.sort();
            paths.extend(entries);
        } else {
            paths.push(path.clone());
        }
    }
    for path in &paths {
        for mesh in load_meshes(Path::new(path)).map_err(|e| e.to_string())? {
            scene.meshes.push(mesh);
        }
    }
    if scene.meshes.is_empty() {
        return Err("no geometry loaded".to_string());
    }

    let mut app = app::App::new(scene, config);
    match args.screenshot {
        Some(path) => app::save_screenshot(&mut app, &path, args.size.0, args.size.1)
            .map_err(|e| e.to_string()),
        None if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() => Err(
            "interactive mode requires a TTY; use --screenshot <output.png> for headless rendering"
                .into(),
        ),
        None => app::run(app).map_err(|e| e.to_string()),
    }
}

fn main() -> ExitCode {
    if std::env::args()
        .skip(1)
        .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
    {
        println!("{}", usage());
        return ExitCode::SUCCESS;
    }
    if std::env::args()
        .skip(1)
        .any(|arg| matches!(arg.as_str(), "--version" | "-V"))
    {
        println!("meshtui {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("meshtui: {e}");
            ExitCode::FAILURE
        }
    }
}
