# AGENTS.md

meshtui is a terminal 3D mesh viewer using the Kitty graphics protocol.
The implementation is a Cargo workspace at the repo root.

## Layout

```
crates/meshtui/         # binary, TUI, commands
crates/meshtui-core/    # mesh, scene, camera, loaders, config
crates/meshtui-render/  # software rasterizer
crates/meshtui-term/    # kitty protocol, themes
```

Each crate has its own `src/`. Do not put the workspace inside a `src/` folder.

## Commands

| Command | Description |
|---------|-------------|
| `cargo test --workspace` | Run all tests |
| `cargo test -p meshtui -- cycle_color` | Run a filtered test |
| `cargo clippy --workspace --all-targets -- -D warnings` | Lint |
| `cargo fmt --all` | Format |
| `cargo build --release` | Binary at `target/release/meshtui` |
| `cargo run -p meshtui -- <path-to-mesh>` | Launch the TUI |

## Style

- Rust 2021, 100-character line length
- Public functions get return types and short doc comments
- Raise/return errors for invalid input; do not unwrap user data
- Never hardcode keybindings; read them from config
- Never hardcode UI colors; use the active theme
