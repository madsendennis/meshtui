# MeshTUI

MeshTUI is a fast terminal 3D mesh viewer written in Rust. It uses a
multithreaded software rasterizer and the Kitty graphics protocol, so it does
not require a GPU, desktop session, or display server.

> **Implementation note:** MeshTUI was auto-converted from an original Python
> implementation to Rust for better performance, a smaller self-contained
> binary, and easier distribution (no Python runtime or dependencies needed).

## Status

Early but functional. Expect breaking changes — the CLI, config keys, and
scene/animation file formats may change between releases.

## Features

- Interactive orbit, zoom, axis presets, orthographic/perspective cameras, and animation
- Camera-anchored key, fill, and rim lighting
- PLY, STL, and OBJ loading, including directories of meshes
- Mesh visibility, color, reset, filtered bulk selection, and regex filtering
- Opaque themed menus over a transparent mesh canvas
- Live Omarchy theme updates with adaptive foreground and selection colors
- Fast shared-memory/local-file image transfer and an SSH-safe direct transfer path
- PNG screenshots in interactive or fully headless environments

## Install

Prebuilt Linux binaries are attached to every
[release](https://github.com/madsendennis/meshtui/releases) — no Rust
toolchain needed.

```bash
# Pin the release version (see https://github.com/madsendennis/meshtui/releases
# for the latest tag). musl = static build, works everywhere; gnu = glibc build.
V=v0.2.0
curl -LO https://github.com/madsendennis/meshtui/releases/download/$V/meshtui-$V-x86_64-unknown-linux-musl.tar.gz
tar xzf meshtui-$V-x86_64-unknown-linux-musl.tar.gz
cd meshtui-*/ && ./install.sh
```

The installer copies `meshtui` to `~/.local/bin` (set `PREFIX=/usr/local`
with sudo for a system-wide install). Or just copy the `meshtui` binary
anywhere in your `PATH` — it is fully self-contained.

To verify a download, fetch `SHA256SUMS` from the same release
(`.../download/$V/SHA256SUMS`) and run `sha256sum -c SHA256SUMS --ignore-missing`.

## Build from source

A stable Rust toolchain is required.

```bash
cargo build --release
```

The binary is written to `target/release/meshtui`.

## Usage

```bash
# Open one or more meshes
meshtui model.ply
meshtui model.stl second.obj

# Load every supported mesh in a directory
meshtui ./models

# Render without a TTY, GPU, or display server
meshtui model.ply \
  --screenshot render.png \
  --size 1920x1080

# Merge an extra configuration over the user config for one run
meshtui model.ply --config config.toml

# Print mesh statistics (MeshLab-style), human-readable or JSON
meshtui info model.ply
meshtui info ./models --json | jq '.totals'

# Render one mesh from several viewpoints (6 axis views, 4 isometric
# diagonals, or an N-step turntable ring) into numbered PNGs
meshtui screenshot model.ply --views all --out-dir shots/
meshtui screenshot model.ply --views ring:12 --prefix gear

# Or all 6 axis views composited into one PNG (2 rows x 3 cols)
meshtui screenshot model.ply --views grid --size 1920x1280

# Full scene control: per-mesh color/alpha/visibility plus camera,
# lighting, wireframe, and background flags
meshtui screenshot \
  "gear=parts/gear.ply:color=#ff8000:alpha=0.9" \
  "base.ply:color=gray" \
  --camera persp --azimuth 30 --elevation 20 --up 0,0,1 \
  --background '#1a1b26' --views grid --out-dir shots

# Render a declarative scene file to one PNG (or open it in the TUI)
meshtui render scene.yaml -o out.png
meshtui scene.yaml            # same scene, interactive

# Render an animation (base scene + scene cuts) to a GIF
meshtui animate cuts.yaml -o out.gif
```

## Headless & agent usage

Every interactive setting is reachable from the terminal, so an agent (or a
script) can inspect a mesh and produce illustrations without a GUI. The tool
is self-describing — explore it instead of reading docs:

```bash
meshtui --capabilities        # machine-readable spec: formats, subcommands,
                              # scene-file & animation keys
meshtui <subcommand> --help   # authoritative flags per subcommand
```

For coding agents, install the skill (a short pointer to `--capabilities`,
so it never goes stale) with the tool itself:

```bash
meshtui skill install          # into ~/.agents/skills/meshtui + detected agents
meshtui skill install --dry-run
meshtui skill print            # or pipe SKILL.md anywhere yourself
meshtui skill uninstall
```

See [SKILL.md](SKILL.md) for the reference. Formats: `.ply .stl .obj .drc
.glb`; a directory loads every supported mesh inside.

```bash
# Machine-readable stats
meshtui info model.ply --json | jq '.meshes[0].bounds'

# Multi-view, all in one PNG
meshtui screenshot model.ply --views grid --out-dir shots
```

### Scene files

`meshtui render scene.yaml -o out.png` renders a YAML scene (meshes with
color/alpha/visibility/transform, camera, lighting, and output
size/background/transparent). The same file passed as `meshtui scene.yaml`
opens interactively, so a scene prepared for an agent looks identical in the
TUI. Example:

```yaml
size: [1600, 1200]
background: "#1a1b26"
camera: { kind: orthographic, view: "+z", up: [0, 1, 0] }
meshes:
  - { path: gear.ply, name: gear, color: "#ff8000", alpha: 0.9 }
  - { path: base.ply, name: base, color: gray }
```

### Animations (scene cuts → GIF)

`meshtui animate cuts.yaml -o out.gif` renders a base scene plus a list of
cuts. Each cut holds N frames (`fps`-timed) and changes only what it names —
camera, per-mesh color/alpha/visibility/transform, light, wireframe — so a
camera-only move is one line. Optional top-level `duration` preserves an exact
GIF playback duration in seconds. `--frames-dir DIR` dumps PNGs instead.

```yaml
size: [800, 600]
fps: 12
camera: { kind: orthographic, view: "+z" }
meshes:
  - { path: gear.ply, name: gear, color: "#ff8000" }
cuts:
  - {}
  - frames: 8
    camera: { azimuth: 90 }
  - frames: 8
    meshes: [{ name: gear, color: red }]
```

### TUI stop-motion recording

Press `qq` to capture the initial state, then press `q` after each pose or
scene change. A checkpoint includes the camera, mesh additions/deletions,
visibility, color/opacity, lighting, and wireframe. Press `Q` to choose the
duration and automatically write the YAML, PNG frames, and GIF; press `Esc`
to cancel. Toggle automatic reframing with `,` (turning it back on reframes
immediately).

## Configuration

On first run, MeshTUI writes a fully commented copy of every default setting
to `~/.config/meshtui/config.toml` (honouring `XDG_CONFIG_HOME`) and loads it
on every subsequent run. Edit that file to change defaults permanently; pass
`--config <file>` to merge an additional file on top for a single run.

Precedence: embedded defaults < user config < `--config`.

```bash
meshtui --print-config                    # dump the merged effective config
meshtui --write-default-config            # (re)write the user defaults file
meshtui --write-default-config=/tmp/c.toml  # or to an explicit path
```

Interactive mode requires a terminal that implements the Kitty graphics
protocol, such as Kitty, Ghostty, or WezTerm.

## Mesh selection and filtering

The side panel always shows the active selection controls:

| Key | Action |
|-----|--------|
| `Up` / `Down` | Move the active row |
| `Space` | Toggle the active mesh |
| `Ctrl+A` | Select every mesh currently shown by the filter |
| `Ctrl+N` | Clear the selection |
| `/` | Set the filter (submit an empty input to clear) |
| `F` | Clear the filter |
| `s` / `S` | Hide or show the selected meshes |
| `c` / `C` | Cycle selected mesh colors |
| `t` / `T` | Cycle selected mesh transparency |
| `x` | Set a custom color |

## Editing the open scene

| Key | Action |
|-----|--------|
| `o` | Add meshes to the scene (file or directory, Tab completes paths) |
| `d` | Delete the selected meshes from the scene |
| `Ctrl+Z` | Restore the most recently deleted meshes |

Deleting applies to the marked meshes, or the active row when nothing is
marked — the same scoping as hide/show. Deleted meshes stay on an undo stack
(32 levels) instead of asking for confirmation.

## `meshtui info`

`meshtui info <mesh...>` prints MeshLab-style statistics for each file or
directory: vertex/face/edge counts, bounding box (min/max/size/diagonal/
center), surface area, signed volume, and the authored color. `--json`
emits the same data machine-readably (with per-mesh `source` and `format`),
and errors become JSON on stderr, so scripts can rely on
`if meshtui info m.ply --json; then ...`.

A normal filter is a case-insensitive substring. Prefix with `re:` for a
case-insensitive regular expression, or `!` to invert:

```text
gear
!tooth
re:^gear_[0-9]+\.(stl|ply)$
!re:tooth
```

Bulk operations are scoped to selected meshes that match the current filter,
so filtered-out selections are never modified accidentally.

Press `?` for the searchable command menu and all configured keybindings.

## Themes

The default `omarchy` theme reads
`~/.config/omarchy/current/theme/kitty.conf`, falling back to the matching
Alacritty theme, a `COLORFGBG` light/dark hint, and finally the built-in dark
theme. Omarchy changes are picked up while MeshTUI is running.

The background, foreground, muted text, accents, and selected-row text all
change together. Built-in alternatives can be selected explicitly:

```toml
[ui.sidepanel]
theme = "github_light" # also: light, nord, dracula, monokai, gruvbox
```

## yazi integration

MeshTUI ships a [yazi](https://yazi-rs.github.io) previewer plugin that renders
PLY/STL/OBJ/DRC/GLB mesh files directly in the file manager's preview pane
(Kitty graphics required). The plugin lives at
[`integrations/yazi/meshtui.yazi`](integrations/yazi/meshtui.yazi/main.lua).

### 1. Install the plugin

Make sure `meshtui` is on your `PATH` (see [Install](#install)), then link or
copy the plugin into yazi's plugin directory:

```bash
mkdir -p ~/.config/yazi/plugins
ln -s /path/to/meshtui/integrations/yazi/meshtui.yazi ~/.config/yazi/plugins/
```

(If you installed from a release tarball, copy the plugin directory out of the
source repo instead.)

### 2. Preview mesh files

Route mesh files to the previewer in `~/.config/yazi/yazi.toml`:

```toml
[plugin]
prepend_previewers = [
  { url = "*.{ply,stl,obj,drc,glb}", run = "meshtui" },
]
prepend_preloaders = [
  { url = "*.{ply,stl,obj,drc,glb}", run = "meshtui" },
]
```

Hover a mesh file and its rendered preview appears in the preview pane;
previews are cached by yazi like image thumbnails. Rendering honors your
MeshTUI config (theme, camera, lighting), and you can pick the preview angle
via `view.default_axis` in `~/.config/meshtui/config.toml`.

Folders keep yazi's built-in file listing — the plugin only renders single
mesh files.

### 3. Open in meshtui

To **open** the hovered mesh file or folder in MeshTUI when you press `Enter`
(or `o`), add an opener to `~/.config/yazi/yazi.toml`:

```toml
[opener]
mesh = [
  { run = 'meshtui %s1', block = true, desc = "View in meshtui" },
]

[open]
prepend_rules = [
  { url = "*.{ply,stl,obj,drc,glb}", use = "mesh" },
  { url = "*/", use = "mesh" },
]
```

`block = true` makes yazi yield the terminal to the MeshTUI TUI and come back
when you quit. The `*/` rule makes `Enter` on a folder open all meshes inside
it; drop that line if you prefer folders to just `cd` into themselves.

## Remote and headless servers

The renderer is CPU-only, so a headless Ubuntu server needs no Vulkan, OpenGL,
X11, or Wayland packages.

For an interactive SSH session, run MeshTUI from a local terminal with Kitty
graphics support and allocate a remote TTY:

```bash
ssh -t server meshtui /data/model.ply
```

MeshTUI detects SSH and embeds each compressed frame directly in the terminal
stream. It does not use remote shared-memory or file paths that the local
terminal cannot access. Terminal multiplexers may need separate graphics
passthrough support.

For jobs, containers, CI, or SSH sessions without a TTY, use `--screenshot`.
Interactive mode fails with a clear message rather than writing graphics
escape sequences into a pipe or log.

## Development

```bash
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all --check
```

## Contributing

If your interests align with the project, contributions are welcome! Feel
free to open issues or pull requests on
[GitHub](https://github.com/madsendennis/meshtui).

## License

MIT
