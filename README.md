# MeshTUI

MeshTUI is a fast terminal 3D mesh viewer written in Rust. It uses a
multithreaded software rasterizer and the Kitty graphics protocol, so it does
not require a GPU, desktop session, or display server.

> **Implementation note:** MeshTUI was auto-converted from an original Python
> implementation to Rust for better performance, a smaller self-contained
> binary, and easier distribution (no Python runtime or dependencies needed).

## Features

- Interactive orbit, zoom, axis presets, orthographic/perspective cameras, and animation
- Camera-anchored key, fill, and rim lighting
- PLY, STL, and OBJ loading, including directories of meshes
- Mesh visibility, color, reset, filtered bulk selection, and regex filtering
- Opaque themed menus over a transparent mesh canvas
- Live Omarchy theme updates with adaptive foreground and selection colors
- Fast shared-memory/local-file image transfer and an SSH-safe direct transfer path
- PNG screenshots in interactive or fully headless environments

## Build

A stable Rust toolchain is required.

```bash
cargo build --release
```

The binary is written to `target/release/meshtui`.

## Usage

```bash
# Open one or more meshes
target/release/meshtui model.ply
target/release/meshtui model.stl second.obj

# Load every supported mesh in a directory
target/release/meshtui ./models

# Render without a TTY, GPU, or display server
target/release/meshtui model.ply \
  --screenshot render.png \
  --size 1920x1080

# Merge a user configuration over the embedded defaults
target/release/meshtui model.ply --config config.toml
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
| `/` | Set or clear the filter |
| `s` / `S` | Hide or show the selected meshes |
| `c` / `C` | Cycle selected mesh colors |
| `x` | Set a custom color |

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

## Remote and headless servers

The renderer is CPU-only, so a headless Ubuntu server needs no Vulkan, OpenGL,
X11, or Wayland packages.

For an interactive SSH session, run MeshTUI from a local terminal with Kitty
graphics support and allocate a remote TTY:

```bash
ssh -t server target/release/meshtui /data/model.ply
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

## License

MIT
