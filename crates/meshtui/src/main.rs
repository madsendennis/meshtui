//! meshtui — terminal 3D mesh viewer (Rust rewrite).
//!
//! Usage:
//!   meshtui <mesh> [more meshes...]   launch the TUI
//!   meshtui <mesh...> --screenshot out.png [--size WxH] [--view <axis>]
//!   meshtui info <mesh...> [--json]   mesh statistics
//!
//! Exit codes: 0 success, 1 error (unlike the Python version, where Typer
//! swallowed the return value and every failure exited 0).

mod app;
mod commands;
mod headless;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use serde::Serialize;

use meshtui_core::config::{self, Config};
use meshtui_core::loaders::{load_path, load_path_detailed};
use meshtui_core::{Color, MeshInfo, Scene, ViewAxis};

use headless::{parse_color, parse_mesh_spec, HeadlessOpts, MeshSpec};
const LONG_ABOUT: &str = "\
Terminal 3D mesh viewer using the Kitty graphics protocol.

Interactive:  meshtui <mesh.ply> [more meshes or directories...]
Headless:     meshtui <mesh...> --screenshot out.png [--size WxH] [--view +x]
Statistics:   meshtui info <mesh...> [--json]

Supported formats: .ply, .stl, .obj, .drc, .glb
Directories load every supported mesh inside (sorted, non-recursive).
Exit codes: 0 success, 1 error.
Config precedence: embedded defaults < ~/.config/meshtui/config.toml < --config.";

/// meshtui — terminal 3D mesh viewer.
#[derive(Debug, Parser)]
#[command(name = "meshtui", version, about, long_about = LONG_ABOUT)]
#[command(subcommand_precedence_over_arg = true)]
struct Cli {
    /// Mesh files or directories to open in the TUI
    #[arg(value_name = "MESH")]
    meshes: Vec<PathBuf>,

    /// Extra TOML config merged over the user config
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Headless render to PNG instead of launching the TUI
    #[arg(long, value_name = "OUT.png")]
    screenshot: Option<PathBuf>,

    /// Screenshot size, WxH (1..=4096 per side)
    #[arg(long, default_value = "1600x1200", value_parser = parse_size)]
    size: (u32, u32),

    /// Initial view axis (only with --screenshot)
    #[arg(long, value_name = "AXIS", value_parser = parse_view)]
    view: Option<ViewAxis>,

    /// Print the merged effective config and exit
    #[arg(long)]
    print_config: bool,

    /// Write all defaults to PATH (default: the user config file) and exit
    #[arg(
        long,
        value_name = "PATH",
        num_args = 0..=1,
        default_missing_value = "",
        value_parser = parse_optional_path
    )]
    write_default_config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Print mesh statistics: counts, bounds, surface area, volume
    Info {
        /// Mesh files or directories
        #[arg(value_name = "MESH", required = true)]
        meshes: Vec<PathBuf>,
        /// Machine-readable JSON output (errors are JSON on stderr)
        #[arg(long)]
        json: bool,
    },
    /// Render one mesh from several viewpoints to numbered PNGs
    Screenshot(Box<ScreenshotArgs>),
}

/// Args for `meshtui screenshot` (boxed in the enum to keep it small).
#[derive(Debug, clap::Args)]
struct ScreenshotArgs {
    /// Mesh files, directories, or specs (`[name=]path[:color=C][:alpha=A][:visible=B]`)
    #[arg(value_name = "MESH", required = true, value_parser = parse_mesh_spec_flag)]
    mesh: Vec<MeshSpec>,

    /// Extra TOML config merged over the user config
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Camera projection
    #[arg(long, value_name = "KIND", value_parser = ["ortho", "persp", "orthographic", "perspective"])]
    camera: Option<String>,

    /// View axis (+x/-x/+y/-y/+z/-z)
    #[arg(long, value_name = "AXIS", value_parser = parse_view)]
    view: Option<ViewAxis>,

    /// Camera azimuth in degrees around the up axis
    #[arg(long, value_name = "DEG", allow_negative_numbers = true)]
    azimuth: Option<f32>,

    /// Camera elevation in degrees above the horizon (default 30)
    #[arg(long, value_name = "DEG", allow_negative_numbers = true)]
    elevation: Option<f32>,

    /// World up vector as X,Y,Z (e.g. --up 0,0,1 for Z-up parts)
    #[arg(long, value_name = "X,Y,Z", value_parser = parse_vec3)]
    up: Option<glam::Vec3>,

    /// Vertical field of view in degrees (perspective only)
    #[arg(long, value_name = "DEG")]
    fov: Option<f32>,

    /// Zoom factor (< 1 in, > 1 out), like the TUI z/Z
    #[arg(long, value_name = "FACTOR")]
    zoom: Option<f32>,

    /// Explicit target→camera distance (overrides the auto fit)
    #[arg(long, value_name = "DIST")]
    distance: Option<f32>,

    /// Light intensity scale 0..=4, like the TUI i/I
    #[arg(long, value_name = "SCALE")]
    light: Option<f32>,

    /// Wireframe thickness in pixels; 0 disables
    #[arg(long, value_name = "PX")]
    wireframe: Option<f32>,

    /// Background as #RRGGBB[AA]; default transparent
    #[arg(long, value_name = "COLOR", value_parser = parse_color_flag)]
    background: Option<Color>,

    /// Viewpoint set: "all", "iso", "grid", or "ring:N"
    #[arg(long, default_value = "all", value_parser = parse_view_set)]
    views: ViewSet,

    /// Write files into DIR (created; default: meshtui_views_<timestamp>)
    #[arg(long, value_name = "DIR")]
    out_dir: Option<PathBuf>,

    /// Common output name prefix (default: first input file stem)
    #[arg(long, value_name = "PREFIX")]
    prefix: Option<String>,

    /// Screenshot size, WxH (1..=4096 per side)
    #[arg(long, default_value = "1600x1200", value_parser = parse_size)]
    size: (u32, u32),
}

/// One planned view: the output file suffix plus the camera direction
/// (target→camera offset). `dir: None` inherits the default pose instead of
/// overriding it.
#[derive(Debug, Clone)]
struct ViewSpec {
    suffix: String,
    dir: Option<glam::Vec3>,
}

/// Which viewpoint set `--views` selects.
#[derive(Debug, Clone)]
enum ViewSet {
    All,
    Iso,
    Ring(u32),
    /// The 6 axis views composited into one 3×2 contact-sheet PNG.
    Grid,
}

fn parse_size(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s.split_once('x').ok_or("--size format: WxH")?;
    let size = (
        w.parse().map_err(|_| "bad width")?,
        h.parse().map_err(|_| "bad height")?,
    );
    if size.0 == 0 || size.1 == 0 || size.0 > 4096 || size.1 > 4096 {
        return Err("--size dimensions must be between 1 and 4096".into());
    }
    Ok(size)
}

fn parse_view(s: &str) -> Result<ViewAxis, String> {
    ViewAxis::parse(s).ok_or("--view must be one of +x, -x, +y, -y, +z, -z".into())
}

fn parse_mesh_spec_flag(s: &str) -> Result<MeshSpec, String> {
    parse_mesh_spec(s)
}

fn parse_color_flag(s: &str) -> Result<Color, String> {
    parse_color(s)
}

fn parse_vec3(s: &str) -> Result<glam::Vec3, String> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != 3 {
        return Err("expected X,Y,Z".into());
    }
    let v: Result<Vec<f32>, _> = parts.iter().map(|p| p.trim().parse::<f32>()).collect();
    let v = v.map_err(|_| "expected three numbers X,Y,Z".to_string())?;
    Ok(glam::Vec3::new(v[0], v[1], v[2]))
}

fn parse_view_set(s: &str) -> Result<ViewSet, String> {
    match s {
        "all" => Ok(ViewSet::All),
        "iso" => Ok(ViewSet::Iso),
        "grid" => Ok(ViewSet::Grid),
        other => {
            let count = other
                .strip_prefix("ring:")
                .and_then(|n| n.parse::<u32>().ok())
                .filter(|n| (2..=64).contains(n))
                .ok_or("--views must be all, iso, grid, or ring:N with N in 2..=64")?;
            Ok(ViewSet::Ring(count))
        }
    }
}

/// Camera back direction for a ring at 30° elevation around `up`, using the
/// same basis math as the spherical orbit.
fn ring_direction(i: u32, n: u32, up: glam::Vec3) -> glam::Vec3 {
    use std::f32::consts::{FRAC_PI_6, TAU};
    let up = up.normalize_or(glam::Vec3::Y);
    let helper = if up.y.abs() > 0.99 {
        glam::Vec3::X
    } else {
        glam::Vec3::Y
    };
    let right = up.cross(helper).normalize_or(glam::Vec3::X);
    let fwd = right.cross(up).normalize_or(glam::Vec3::Z);
    let theta = i as f32 / n as f32 * TAU;
    let (sin_e, cos_e) = FRAC_PI_6.sin_cos();
    right * (cos_e * theta.cos()) + fwd * (cos_e * theta.sin()) + up * sin_e
}

/// Expand the selected set into concrete view plans. Ring and iso names are
/// zero-padded so files sort in view order.
fn view_specs(set: &ViewSet, up: glam::Vec3) -> Vec<ViewSpec> {
    match set {
        ViewSet::All => [
            (ViewAxis::PosX, "plus_x"),
            (ViewAxis::NegX, "minus_x"),
            (ViewAxis::PosY, "plus_y"),
            (ViewAxis::NegY, "minus_y"),
            (ViewAxis::PosZ, "plus_z"),
            (ViewAxis::NegZ, "minus_z"),
        ]
        .into_iter()
        .map(|(axis, suffix)| ViewSpec {
            suffix: suffix.into(),
            dir: Some(axis.direction().normalize()),
        })
        .collect(),
        ViewSet::Iso => [
            (glam::Vec3::new(1.0, 1.0, 1.0), "iso_0"),
            (glam::Vec3::new(-1.0, 1.0, 1.0), "iso_1"),
            (glam::Vec3::new(1.0, 1.0, -1.0), "iso_2"),
            (glam::Vec3::new(-1.0, 1.0, -1.0), "iso_3"),
        ]
        .into_iter()
        .map(|(dir, suffix)| ViewSpec {
            suffix: suffix.into(),
            dir: Some(dir.normalize()),
        })
        .collect(),
        ViewSet::Ring(n) => {
            let width = (n - 1).to_string().len();
            (0..*n)
                .map(|i| ViewSpec {
                    suffix: format!("ring_{i:0width$}"),
                    dir: Some(ring_direction(i, *n, up)),
                })
                .collect()
        }
        ViewSet::Grid => Vec::new(), // handled separately: composite, not files
    }
}

/// clap's built-in PathBuf parser rejects the empty `default_missing_value`
/// used for a bare `--write-default-config`; this one passes it through so
/// the empty path can mean "the default user config location".
fn parse_optional_path(s: &str) -> Result<PathBuf, String> {
    Ok(PathBuf::from(s))
}

/// Validate flag combinations that clap cannot express. Kept separate from
/// `run` so tests can cover the combinations without side effects.
fn validate_cli(cli: &Cli) -> Result<(), String> {
    if cli.command.is_some() {
        return Ok(());
    }
    if cli.meshes.is_empty() && !cli.print_config && cli.write_default_config.is_none() {
        return Err("no meshes given (see --help)".into());
    }
    if cli.view.is_some() && cli.screenshot.is_none() {
        return Err("--view only applies together with --screenshot".into());
    }
    Ok(())
}

/// One mesh in an `info` report: where it came from plus its statistics.
#[derive(Serialize)]
struct MeshEntry {
    source: PathBuf,
    format: String,
    #[serde(flatten)]
    info: MeshInfo,
}

#[derive(Serialize)]
struct InfoTotals {
    meshes: usize,
    vertices: usize,
    faces: usize,
    surface_area: f64,
    signed_volume: f64,
}

#[derive(Serialize)]
struct InfoReport {
    meshes: Vec<MeshEntry>,
    totals: InfoTotals,
}

/// `meshtui info`: MeshLab-style per-mesh statistics, human-readable or JSON.
fn run_info(paths: &[PathBuf], json: bool) -> Result<(), String> {
    let mut entries = Vec::new();
    for path in paths {
        let loaded = load_path_detailed(path).map_err(|e| {
            if json {
                // Machine-readable error on stderr keeps stdout parseable;
                // the empty message tells main not to repeat it as text.
                let message = e.to_string();
                let quoted =
                    serde_json::to_string(&message).unwrap_or_else(|_| format!("{message:?}"));
                eprintln!("{{\"error\": {quoted}}}");
                return String::new();
            }
            e.to_string()
        })?;
        for (source, mesh) in loaded {
            let format = if source.is_file() {
                source
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_ascii_lowercase())
                    .unwrap_or_default()
            } else {
                String::new()
            };
            entries.push(MeshEntry {
                source,
                format,
                info: MeshInfo::from_mesh(&mesh),
            });
        }
    }
    let totals = InfoTotals {
        meshes: entries.len(),
        vertices: entries.iter().map(|e| e.info.vertices).sum(),
        faces: entries.iter().map(|e| e.info.faces).sum(),
        surface_area: entries.iter().map(|e| e.info.surface_area).sum(),
        signed_volume: entries.iter().map(|e| e.info.signed_volume).sum(),
    };
    if json {
        let report = InfoReport {
            meshes: entries,
            totals,
        };
        let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
        println!("{text}");
    } else {
        print_human_info(&entries, &totals);
    }
    Ok(())
}

fn print_human_info(entries: &[MeshEntry], totals: &InfoTotals) {
    for (i, entry) in entries.iter().enumerate() {
        if i > 0 {
            println!();
        }
        let info = &entry.info;
        println!("{} ({})", entry.source.display(), entry.format);
        let stem = entry
            .source
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if info.name != stem {
            // Multi-geometry files name each mesh; show it when it differs.
            println!("  name:          {}", info.name);
        }
        println!("  vertices:      {}", info.vertices);
        println!("  faces:         {}", info.faces);
        println!("  edges:         {}", info.edges);
        match &info.bounds {
            Some(bounds) => {
                let [sx, sy, sz] = bounds.size;
                println!(
                    "  bounds min:    ({:.3}, {:.3}, {:.3})",
                    bounds.min[0], bounds.min[1], bounds.min[2]
                );
                println!(
                    "  bounds max:    ({:.3}, {:.3}, {:.3})",
                    bounds.max[0], bounds.max[1], bounds.max[2]
                );
                println!(
                    "  bounds size:   {sx:.3} x {sy:.3} x {sz:.3}  (diagonal {:.3})",
                    bounds.diagonal
                );
                println!(
                    "  center:        ({:.3}, {:.3}, {:.3})",
                    bounds.center[0], bounds.center[1], bounds.center[2]
                );
            }
            None => println!("  bounds:        (empty mesh)"),
        }
        println!("  surface area:  {:.6}", info.surface_area);
        println!(
            "  volume:        {:.6} (signed; meaningful for closed meshes)",
            info.signed_volume
        );
        match info.authored_color {
            Some([r, g, b, a]) => {
                println!("  color:         authored [{r:.2}, {g:.2}, {b:.2}, {a:.2}]")
            }
            None => println!("  color:         default (no authored color)"),
        }
    }
    if entries.len() > 1 {
        println!();
        println!(
            "totals: {} meshes, {} vertices, {} faces, surface area {:.6}",
            totals.meshes, totals.vertices, totals.faces, totals.surface_area
        );
    }
}

/// `meshtui screenshot`: render mesh(es) from several viewpoints into
/// numbered PNGs. Per-mesh colors/visibility/alpha and the camera/light/
/// wireframe flags all apply, so the terminal can set up the full scene the
/// TUI would show.
#[allow(clippy::too_many_arguments)]
fn run_screenshot(
    specs: &[MeshSpec],
    config_path: Option<&std::path::Path>,
    opts: &HeadlessOpts,
    set: &ViewSet,
    out_dir: Option<&std::path::Path>,
    prefix: Option<&str>,
    size: (u32, u32),
) -> Result<(), String> {
    let scene = headless::load_scene(specs)?;
    let config = Config::load_effective(config_path).map_err(|e| e.to_string())?;
    let up = opts
        .up
        .unwrap_or_else(|| glam::Vec3::from(config.view.up_vectors[0]));

    let mut app = app::App::new(scene, config);
    headless::apply_headless(&mut app, opts);

    let prefix = prefix.map(String::from).unwrap_or_else(|| {
        Path::new(&specs[0].path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("mesh")
            .to_string()
    });
    let dir = match out_dir {
        Some(dir) => dir.to_path_buf(),
        None => {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            std::env::current_dir()
                .map_err(|e| e.to_string())?
                .join(format!("meshtui_views_{ts}"))
        }
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;

    if matches!(set, ViewSet::Grid) {
        let path = dir.join(format!("{prefix}_grid.png"));
        write_view_grid(&mut app, &path, size, up, opts)?;
        println!("{}", path.display());
        return Ok(());
    }

    for spec in view_specs(set, up) {
        pose_camera(&mut app, &spec);
        let path = dir.join(format!("{prefix}_{}.png", spec.suffix));
        save_shot(&mut app, &path, size, opts)?;
        println!("{}", path.display());
    }
    Ok(())
}

/// Save a frame, compositing over `--background` unless transparent.
fn save_shot(
    app: &mut app::App,
    path: &std::path::Path,
    size: (u32, u32),
    opts: &HeadlessOpts,
) -> Result<(), String> {
    let frame = app
        .render_frame(size.0, size.1)
        .ok_or("nothing to screenshot: all meshes hidden")?;
    let pixels = composite(frame.pixels, frame.width, opts);
    write_png(path, &pixels, frame.width, frame.height)
}

/// Alpha-composite a rendered frame over the background color (no-op when the
/// background is transparent / unset).
fn composite(mut pixels: Vec<u8>, _width: u32, opts: &HeadlessOpts) -> Vec<u8> {
    let Some(bg) = opts.background.filter(|_| !opts.transparent) else {
        return pixels;
    };
    let (br, bg_g, bb) = (
        bg[0] as f32 / 255.0,
        bg[1] as f32 / 255.0,
        bg[2] as f32 / 255.0,
    );
    for px in pixels.as_chunks_mut::<4>().0.iter_mut() {
        let a = px[3] as f32 / 255.0;
        if a < 1.0 {
            px[0] = (px[0] as f32 * a + br * 255.0 * (1.0 - a)).round() as u8;
            px[1] = (px[1] as f32 * a + bg_g * 255.0 * (1.0 - a)).round() as u8;
            px[2] = (px[2] as f32 * a + bb * 255.0 * (1.0 - a)).round() as u8;
            px[3] = 255;
        }
    }
    pixels
}

/// Pose the camera for a planned view (no-op when the spec has no override).
fn pose_camera(app: &mut app::App, spec: &ViewSpec) {
    let Some(dir_vec) = spec.dir else { return };
    let up = if dir_vec.dot(app.camera.up()).abs() > 0.99 {
        // View along the up axis: pick any perpendicular up.
        if dir_vec.dot(glam::Vec3::Y).abs() > 0.99 {
            glam::Vec3::Z
        } else {
            glam::Vec3::Y
        }
    } else {
        app.camera.up()
    };
    app.camera.orientation = meshtui_core::camera::look_rotation(dir_vec, up);
}

/// The 6 axis views composited into one 3×2 contact sheet. Each cell is
/// rendered at (W/3, H/2) so a landscape `--size WxH` yields roughly square
/// cells; the PNG is exactly WxH.
fn write_view_grid(
    app: &mut app::App,
    path: &std::path::Path,
    size: (u32, u32),
    up: glam::Vec3,
    opts: &HeadlessOpts,
) -> Result<(), String> {
    let (w, h) = size;
    let cell_w = (w / 3).max(1);
    let cell_h = (h / 2).max(1);
    app.set_aspect(cell_w as f32 / cell_h as f32);
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    for (i, spec) in view_specs(&ViewSet::All, up).iter().enumerate() {
        pose_camera(app, spec);
        let frame = app
            .render_frame(cell_w, cell_h)
            .ok_or("nothing to screenshot: all meshes hidden")?;
        // Top row: +x -x +y; bottom row: -y +z -z (spec order is already the
        // axis order, so place them 3 per row).
        let col = (i % 3) as u32;
        let row = (i / 3) as u32;
        blit(&mut pixels, w, &frame, col * cell_w, row * cell_h);
    }
    let pixels = composite(pixels, w, opts);
    write_png(path, &pixels, w, h)
}

/// Copy `frame` into the RGBA `canvas` (width `canvas_w`) at offset (ox, oy),
/// clipping to the canvas.
fn blit(canvas: &mut [u8], canvas_w: u32, frame: &meshtui_render::Frame, ox: u32, oy: u32) {
    let canvas_h = canvas.len() as u32 / canvas_w / 4;
    for y in 0..frame.height.min(canvas_h.saturating_sub(oy)) {
        let dst = ((oy + y) * canvas_w + ox) as usize * 4;
        let src = (y * frame.width) as usize * 4;
        let width = (frame.width.min(canvas_w.saturating_sub(ox)) * 4) as usize;
        canvas[dst..dst + width].copy_from_slice(&frame.pixels[src..src + width]);
    }
}

/// Write RGBA pixels to a PNG.
fn write_png(path: &std::path::Path, pixels: &[u8], w: u32, h: u32) -> Result<(), String> {
    image::write_buffer_with_format(
        &mut std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| e.to_string())?),
        pixels,
        w,
        h,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .map_err(|e| e.to_string())
}

fn run(cli: Cli) -> Result<(), String> {
    validate_cli(&cli)?;

    match &cli.command {
        Some(Commands::Info { meshes, json }) => return run_info(meshes, *json),
        Some(Commands::Screenshot(args)) => {
            let opts = HeadlessOpts {
                camera_kind: args.camera.as_deref().map(|k| match k {
                    "ortho" | "orthographic" => "orthographic",
                    _ => "perspective",
                }),
                view: args.view,
                azimuth: args.azimuth,
                elevation: args.elevation,
                up: args.up,
                fov: args.fov,
                zoom: args.zoom,
                distance: args.distance,
                light: args.light,
                wireframe: args.wireframe,
                background: args.background.map(|c| {
                    [
                        (c[0] * 255.0) as u8,
                        (c[1] * 255.0) as u8,
                        (c[2] * 255.0) as u8,
                        (c[3] * 255.0) as u8,
                    ]
                }),
                transparent: false,
            };
            return run_screenshot(
                &args.mesh,
                args.config.as_deref(),
                &opts,
                &args.views,
                args.out_dir.as_deref(),
                args.prefix.as_deref(),
                args.size,
            );
        }
        None => {}
    }

    // Config-only commands run without meshes.
    if let Some(path) = &cli.write_default_config {
        let path = if path.as_os_str().is_empty() {
            config::user_config_path()
                .ok_or("could not determine user config directory (is HOME set?)")?
        } else {
            path.clone()
        };
        config::write_default_config(&path).map_err(|e| e.to_string())?;
        println!("wrote default config to {}", path.display());
        return Ok(());
    }
    if cli.print_config {
        let text =
            config::effective_config_toml(cli.config.as_deref()).map_err(|e| e.to_string())?;
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

    let config = Config::load_effective(cli.config.as_deref()).map_err(|e| e.to_string())?;

    let mut scene = Scene::new();
    for path in &cli.meshes {
        for mesh in load_path(path).map_err(|e| e.to_string())? {
            scene.meshes.push(mesh);
        }
    }
    if scene.meshes.is_empty() {
        return Err("no geometry loaded".to_string());
    }

    let mut app = app::App::new(scene, config);
    if let Some(axis) = cli.view {
        app.camera.set_view_axis(axis);
    }
    match cli.screenshot {
        Some(path) => {
            // Fit the camera to the actual output aspect before rendering.
            app.set_aspect(cli.size.0 as f32 / cli.size.1 as f32);
            app::save_screenshot(&mut app, &path, cli.size.0, cli.size.1).map_err(|e| e.to_string())
        }
        None if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() => Err(
            "interactive mode requires a TTY; use --screenshot <output.png> for headless rendering"
                .into(),
        ),
        None => app::run(app).map_err(|e| e.to_string()),
    }
}

fn main() -> ExitCode {
    // Rust ignores SIGPIPE, so a closed stdout (e.g. `meshtui info ... |
    // head`) panics on println!. Restore the default: die silently like a
    // normal Unix tool.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            // Preserve the documented exit codes: 0 for --help/--version
            // output, 1 for usage errors (clap itself would exit 2).
            if matches!(
                e.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) {
                print!("{e}");
                return ExitCode::SUCCESS;
            }
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // Empty message: already reported (e.g. as JSON by `info`).
            if !e.is_empty() {
                eprintln!("meshtui: {e}");
            }
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(argv: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(argv.iter().copied())
    }

    #[test]
    fn view_parses_axis() {
        let cli = parse(&["meshtui", "m.ply", "--screenshot", "o.png", "--view", "+x"]).unwrap();
        assert_eq!(cli.view, Some(ViewAxis::PosX));
        assert!(validate_cli(&cli).is_ok());
    }

    #[test]
    fn view_rejects_bad_axis() {
        assert!(parse(&["meshtui", "m.ply", "--screenshot", "o.png", "--view", "up"]).is_err());
    }

    #[test]
    fn view_requires_screenshot() {
        let cli = parse(&["meshtui", "m.ply", "--view", "+z"]).unwrap();
        assert!(validate_cli(&cli).is_err());
    }

    #[test]
    fn screenshot_without_view_is_fine() {
        let cli = parse(&["meshtui", "m.ply", "--screenshot", "o.png"]).unwrap();
        assert!(cli.view.is_none());
        assert!(validate_cli(&cli).is_ok());
    }

    #[test]
    fn size_parses_and_validates_bounds() {
        let cli = parse(&[
            "meshtui",
            "m.ply",
            "--screenshot",
            "o.png",
            "--size",
            "800x600",
        ])
        .unwrap();
        assert_eq!(cli.size, (800, 600));
        assert!(parse(&["meshtui", "m.ply", "--size", "0x10"]).is_err());
        assert!(parse(&["meshtui", "m.ply", "--size", "9999x10"]).is_err());
        assert!(parse(&["meshtui", "m.ply", "--size", "bad"]).is_err());
    }

    #[test]
    fn info_subcommand_parses_with_json_flag() {
        let cli = parse(&["meshtui", "info", "a.ply", "b_dir", "--json"]).unwrap();
        match cli.command {
            Some(Commands::Info { meshes, json }) => {
                assert_eq!(meshes, [PathBuf::from("a.ply"), PathBuf::from("b_dir")]);
                assert!(json);
            }
            other => panic!("info must parse as a subcommand, not {other:?}"),
        }
    }

    #[test]
    fn screenshot_subcommand_parses_with_view_flags() {
        let cli = parse(&[
            "meshtui",
            "screenshot",
            "m.ply",
            "--views",
            "ring:8",
            "--out-dir",
            "/tmp/shots",
            "--prefix",
            "gear",
            "--size",
            "800x600",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Screenshot(args)) => {
                assert_eq!(args.mesh.len(), 1);
                assert_eq!(args.mesh[0].path, "m.ply");
                assert!(matches!(args.views, ViewSet::Ring(8)));
                assert_eq!(args.out_dir, Some(PathBuf::from("/tmp/shots")));
                assert_eq!(args.prefix.as_deref(), Some("gear"));
                assert_eq!(args.size, (800, 600));
            }
            other => panic!("screenshot must parse as a subcommand, not {other:?}"),
        }
    }
    #[test]
    fn screenshot_parses_mesh_specs_and_camera_flags() {
        let cli = parse(&[
            "meshtui",
            "screenshot",
            "gear=a.ply:color=#ff0000:alpha=0.5",
            "b.ply:visible=false",
            "--camera",
            "persp",
            "--azimuth",
            "45",
            "--up",
            "0,0,1",
            "--zoom",
            "0.8",
            "--views",
            "grid",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Screenshot(args)) => {
                assert_eq!(args.mesh.len(), 2);
                assert_eq!(args.mesh[0].name.as_deref(), Some("gear"));
                assert_eq!(args.mesh[0].color, Some([1.0, 0.0, 0.0, 1.0]));
                assert_eq!(args.mesh[1].visible, Some(false));
                assert_eq!(args.camera.as_deref(), Some("persp"));
                assert_eq!(args.azimuth, Some(45.0));
                assert_eq!(args.up, Some(glam::Vec3::Z));
                assert_eq!(args.zoom, Some(0.8));
                assert!(matches!(args.views, ViewSet::Grid));
            }
            other => panic!("expected screenshot, got {other:?}"),
        }
    }

    #[test]
    fn empty_invocation_is_rejected() {
        let cli = parse(&["meshtui"]).unwrap();
        assert!(validate_cli(&cli).is_err());
    }

    #[test]
    fn write_default_config_path_is_optional() {
        let cli = parse(&["meshtui", "--write-default-config"]).unwrap();
        assert_eq!(cli.write_default_config, Some(PathBuf::new()));
        let cli = parse(&["meshtui", "--write-default-config", "/tmp/x.toml"]).unwrap();
        assert_eq!(cli.write_default_config, Some(PathBuf::from("/tmp/x.toml")));
    }

    #[test]
    fn view_set_parses_all_iso_ring() {
        assert!(matches!(parse_view_set("all").unwrap(), ViewSet::All));
        assert!(matches!(parse_view_set("iso").unwrap(), ViewSet::Iso));
        assert!(matches!(parse_view_set("grid").unwrap(), ViewSet::Grid));
        assert!(matches!(
            parse_view_set("ring:8").unwrap(),
            ViewSet::Ring(8)
        ));
        assert!(parse_view_set("ring:1").is_err());
        assert!(parse_view_set("ring:65").is_err());
        assert!(parse_view_set("ring:x").is_err());
        assert!(parse_view_set("front").is_err());
    }

    #[test]
    fn blit_clips_and_copies() {
        // 2x1 red frame onto a 4x2 canvas at (3, 1): the rightmost column
        // clips off the 4-wide edge.
        let mut canvas = vec![0u8; 4 * 2 * 4];
        let frame = meshtui_render::Frame {
            width: 2,
            height: 1,
            pixels: vec![255, 0, 0, 255, 255, 0, 0, 255],
        };
        blit(&mut canvas, 4, &frame, 3, 1);
        // Only the first frame pixel lands (col 3, row 1); col 4 is clipped.
        let (col, row) = (3usize, 1usize);
        let px = (row * 4 + col) * 4;
        assert_eq!(&canvas[px..px + 4], &[255, 0, 0, 255]);
        assert!(canvas[..px].iter().all(|&b| b == 0), "nothing above offset");
    }

    #[test]
    fn view_specs_cover_the_axis_set() {
        let specs = view_specs(&ViewSet::All, glam::Vec3::Y);
        assert_eq!(specs.len(), 6);
        let suffixes: Vec<&str> = specs.iter().map(|s| s.suffix.as_str()).collect();
        assert_eq!(
            suffixes,
            ["plus_x", "minus_x", "plus_y", "minus_y", "plus_z", "minus_z"]
        );
        // Opposite axes are anti-parallel.
        let dirs: Vec<glam::Vec3> = specs.iter().filter_map(|s| s.dir).collect();
        assert!(dirs[0].dot(dirs[1]) < -0.99);
    }

    #[test]
    fn ring_specs_are_evenly_spaced_at_elevation() {
        let specs = view_specs(&ViewSet::Ring(8), glam::Vec3::Y);
        assert_eq!(specs.len(), 8);
        assert_eq!(specs[0].suffix, "ring_0");
        let dirs: Vec<glam::Vec3> = specs.iter().filter_map(|s| s.dir).collect();
        for dir in &dirs {
            // 30° elevation: direction·up = sin(30°).
            assert!(
                (dir.dot(glam::Vec3::Y) - 0.5).abs() < 1e-5,
                "ring stays at 30° elevation"
            );
            assert!((dir.length() - 1.0).abs() < 1e-5);
        }
        // Adjacent views are 45° apart in azimuth and the ring closes.
        // Compare azimuth of the horizontal projection, not dot products of
        // the tilted vectors; the step may run either way around the axis.
        let azimuth = |d: glam::Vec3| d.z.atan2(d.x).rem_euclid(std::f32::consts::TAU);
        let step = (azimuth(dirs[1]) - azimuth(dirs[0])).rem_euclid(std::f32::consts::TAU);
        let expected = 45f32.to_radians();
        assert!(
            (step - expected).abs() < 1e-4
                || (step + expected - std::f32::consts::TAU).abs() < 1e-4,
            "45° azimuth step either way, got {}",
            step.to_degrees()
        );
        let wrap = (azimuth(dirs[0]) + std::f32::consts::TAU - azimuth(dirs[7]))
            .rem_euclid(std::f32::consts::TAU);
        assert!(
            (wrap - expected).abs() < 1e-4
                || (wrap + expected - std::f32::consts::TAU).abs() < 1e-4,
            "ring closes, got {}",
            wrap.to_degrees()
        );
    }

    #[test]
    fn ring_specs_zero_pad_for_sort_order() {
        let specs = view_specs(&ViewSet::Ring(12), glam::Vec3::Y);
        assert_eq!(specs[1].suffix, "ring_01");
        assert_eq!(specs[11].suffix, "ring_11");
    }

    #[test]
    fn human_info_renders_meshlab_style_fields() {
        let mut mesh = meshtui_core::Mesh::new("tri");
        mesh.positions = vec![glam::Vec3::ZERO, glam::Vec3::X, glam::Vec3::Y];
        mesh.indices = vec![0, 1, 2];
        let entry = MeshEntry {
            source: PathBuf::from("tri.ply"),
            format: "ply".into(),
            info: MeshInfo::from_mesh(&mesh),
        };
        let totals = InfoTotals {
            meshes: 1,
            vertices: 3,
            faces: 1,
            surface_area: 0.5,
            signed_volume: 0.0,
        };
        // Must not panic; the JSON must carry the MeshLab-style fields.
        print_human_info(std::slice::from_ref(&entry), &totals);
        let json = serde_json::to_string(&InfoReport {
            meshes: vec![entry],
            totals,
        })
        .unwrap();
        assert!(json.contains("\"vertices\":3"));
        assert!(json.contains("\"edges\":3"));
        assert!(json.contains("\"diagonal\""));
    }
}
