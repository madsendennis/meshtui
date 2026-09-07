//! Theme resolution, including live Omarchy integration.
//!
//! Resolution chain when `[ui.sidepanel].theme = "omarchy"`:
//! 1. the current omarchy theme dir, preferring the running terminal's own
//!    config file, falling back across `kitty.conf`, `ghostty.conf`,
//!    `alacritty.toml`, `foot.ini` and `colors.toml`
//! 2. `COLORFGBG` light/dark hint (useful across SSH)
//! 3. built-in nord defaults
//!
//! The theme dir is `~/.local/state/omarchy/current/theme` on newer Omarchy
//! versions and `~/.config/omarchy/current/theme` (a symlink) on older ones.
//! [`ThemeWatcher`] watches the resolved dir; when `omarchy-theme-set`
//! rewrites or retargets it, the app re-resolves live.

use std::collections::HashMap;
use std::path::PathBuf;

use ratatui::style::Color as TColor;

/// UI palette consumed by the TUI renderer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub background: TColor,
    pub foreground: TColor,
    pub accent: TColor,
    pub selection: TColor,
    pub selection_foreground: TColor,
    pub muted: TColor,
}

impl Theme {
    /// Built-in fallback (nord-ish).
    pub fn builtin(name: &str) -> Self {
        match name {
            "dracula" => Self {
                background: TColor::Rgb(40, 42, 54),
                foreground: TColor::Rgb(248, 248, 242),
                accent: TColor::Rgb(255, 121, 198),
                selection: TColor::Rgb(68, 71, 90),
                selection_foreground: TColor::Rgb(248, 248, 242),
                muted: TColor::Rgb(98, 114, 164),
            },
            "monokai" => Self {
                background: TColor::Rgb(39, 40, 34),
                foreground: TColor::Rgb(248, 248, 242),
                accent: TColor::Rgb(166, 226, 46),
                selection: TColor::Rgb(73, 72, 62),
                selection_foreground: TColor::Rgb(248, 248, 242),
                muted: TColor::Rgb(117, 113, 94),
            },
            "gruvbox" => Self {
                background: TColor::Rgb(40, 40, 40),
                foreground: TColor::Rgb(235, 219, 178),
                accent: TColor::Rgb(250, 189, 47),
                selection: TColor::Rgb(80, 73, 69),
                selection_foreground: TColor::Rgb(235, 219, 178),
                muted: TColor::Rgb(146, 131, 116),
            },
            "github_light" | "light" => Self {
                background: TColor::Rgb(255, 255, 255),
                foreground: TColor::Rgb(31, 35, 40),
                accent: TColor::Rgb(9, 105, 218),
                selection: TColor::Rgb(221, 244, 255),
                selection_foreground: TColor::Rgb(31, 35, 40),
                muted: TColor::Rgb(87, 96, 106),
            },
            _ => Self {
                // nord / default
                background: TColor::Rgb(46, 52, 64),
                foreground: TColor::Rgb(216, 222, 233),
                accent: TColor::Rgb(136, 192, 208),
                selection: TColor::Rgb(67, 76, 94),
                selection_foreground: TColor::Rgb(236, 239, 244),
                muted: TColor::Rgb(129, 161, 193),
            },
        }
    }

    /// True when the background is dark (drives render-side color choices
    /// such as the wireframe default).
    pub fn is_dark(&self) -> bool {
        match self.background {
            TColor::Rgb(r, g, b) => {
                0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32 <= 128.0
            }
            _ => true,
        }
    }
}

fn omarchy_theme_dir() -> Option<PathBuf> {
    let home = dirs_home()?;
    // Newer Omarchy versions stage the current theme under the state dir;
    // older ones symlinked it under the config dir.
    [
        home.join(".local/state/omarchy/current/theme"),
        home.join(".config/omarchy/current/theme"),
    ]
    .into_iter()
    .find(|dir| dir.exists())
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Parse a kitty.conf: `key value` lines, `#rrggbb` colors.
pub fn parse_kitty_conf(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once(char::is_whitespace) {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    map
}

fn parse_hex(s: &str) -> Option<TColor> {
    let s = s.strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some(TColor::Rgb(
        ((v >> 16) & 0xff) as u8,
        ((v >> 8) & 0xff) as u8,
        (v & 0xff) as u8,
    ))
}

fn contrast_foreground(background: TColor) -> TColor {
    match background {
        TColor::Rgb(r, g, b)
            if 0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32 > 128.0 =>
        {
            TColor::Rgb(31, 35, 40)
        }
        _ => TColor::Rgb(236, 239, 244),
    }
}

fn blend(from: TColor, to: TColor, amount: f32) -> TColor {
    match (from, to) {
        (TColor::Rgb(fr, fg, fb), TColor::Rgb(tr, tg, tb)) => {
            let channel = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * amount).round() as u8;
            TColor::Rgb(channel(fr, tr), channel(fg, tg), channel(fb, tb))
        }
        _ => to,
    }
}

fn theme_from_kitty(map: &HashMap<String, String>) -> Option<Theme> {
    let get = |k: &str| map.get(k).and_then(|v| parse_hex(v));
    let background = get("background")?;
    let foreground = get("foreground").unwrap_or_else(|| contrast_foreground(background));
    // Accent: prefer cursor/selection or the ansi blue/cyan.
    let accent = get("cursor")
        .or_else(|| get("color4"))
        .or_else(|| get("color6"))
        .unwrap_or(TColor::Cyan);
    let selection =
        get("selection_background").unwrap_or_else(|| blend(background, foreground, 0.18));
    let selection_foreground =
        get("selection_foreground").unwrap_or_else(|| contrast_foreground(selection));
    let muted = get("color8").unwrap_or_else(|| blend(background, foreground, 0.6));
    Some(Theme {
        background,
        foreground,
        accent,
        selection,
        selection_foreground,
        muted,
    })
}

/// Parse ghostty.conf: `key = value` lines, `#rrggbb` colors, indexed
/// `palette = N=#rrggbb` entries.
fn theme_from_ghostty(text: &str) -> Option<Theme> {
    let mut map: HashMap<String, String> = HashMap::new();
    let mut palette: HashMap<u8, TColor> = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if key == "palette" {
            if let Some((index, hex)) = value.split_once('=') {
                if let (Ok(index), Some(color)) =
                    (index.trim().parse::<u8>(), parse_hex(hex.trim()))
                {
                    palette.insert(index, color);
                }
            }
        } else {
            map.insert(key.to_string(), value.to_string());
        }
    }
    let get = |k: &str| map.get(k).and_then(|v| parse_hex(v));
    let background = get("background")?;
    let foreground = get("foreground").unwrap_or_else(|| contrast_foreground(background));
    let accent = get("cursor-color")
        .or_else(|| palette.get(&4).copied())
        .or_else(|| palette.get(&6).copied())
        .unwrap_or(TColor::Cyan);
    let selection =
        get("selection-background").unwrap_or_else(|| blend(background, foreground, 0.18));
    let selection_foreground =
        get("selection-foreground").unwrap_or_else(|| contrast_foreground(selection));
    let muted = palette
        .get(&8)
        .copied()
        .unwrap_or_else(|| blend(background, foreground, 0.6));
    Some(Theme {
        background,
        foreground,
        accent,
        selection,
        selection_foreground,
        muted,
    })
}

/// Parse foot.ini: INI sections, hex colors without a leading `#`. Colors
/// live in `[colors]`, `[colors-dark]` or `[colors-light]`.
fn theme_from_foot(text: &str) -> Option<Theme> {
    let mut sections: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.trim().to_string();
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            sections
                .entry(section.clone())
                .or_default()
                .insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    let map = sections
        .get("colors")
        .or_else(|| sections.get("colors-dark"))
        .or_else(|| sections.get("colors-light"))?;
    let get = |k: &str| {
        map.get(k)
            .and_then(|v| parse_hex(v).or_else(|| parse_hex(&format!("#{v}"))))
    };
    let background = get("background")?;
    let foreground = get("foreground").unwrap_or_else(|| contrast_foreground(background));
    // `cursor = <text> <cursor>`; the cursor color is the second token.
    let cursor = map
        .get("cursor")
        .and_then(|v| v.split_whitespace().nth(1))
        .and_then(|v| parse_hex(v).or_else(|| parse_hex(&format!("#{v}"))));
    let accent = cursor
        .or_else(|| get("regular4"))
        .or_else(|| get("regular6"))
        .unwrap_or(TColor::Cyan);
    let selection =
        get("selection-background").unwrap_or_else(|| blend(background, foreground, 0.18));
    let selection_foreground =
        get("selection-foreground").unwrap_or_else(|| contrast_foreground(selection));
    let muted = get("bright0").unwrap_or_else(|| blend(background, foreground, 0.6));
    Some(Theme {
        background,
        foreground,
        accent,
        selection,
        selection_foreground,
        muted,
    })
}

/// Parse omarchy's own colors.toml (flat `background = "#rrggbb"` keys).
fn theme_from_colors_toml(text: &str) -> Option<Theme> {
    let doc: toml::Value = toml::from_str(text).ok()?;
    let get = |k: &str| doc.get(k).and_then(|v| v.as_str()).and_then(parse_hex);
    let background = get("background")?;
    let foreground = get("foreground").unwrap_or_else(|| contrast_foreground(background));
    let accent = get("accent")
        .or_else(|| get("blue"))
        .or_else(|| get("cyan"))
        .unwrap_or(TColor::Cyan);
    let selection = get("selection").unwrap_or_else(|| blend(background, foreground, 0.18));
    let selection_foreground = contrast_foreground(selection);
    let muted = get("muted").unwrap_or_else(|| blend(background, foreground, 0.6));
    Some(Theme {
        background,
        foreground,
        accent,
        selection,
        selection_foreground,
        muted,
    })
}

fn parse_theme_file(name: &str, text: &str) -> Option<Theme> {
    match name {
        "kitty.conf" => theme_from_kitty(&parse_kitty_conf(text)),
        "ghostty.conf" => theme_from_ghostty(text),
        "alacritty.toml" => theme_from_alacritty(text),
        "foot.ini" => theme_from_foot(text),
        "colors.toml" => theme_from_colors_toml(text),
        _ => None,
    }
}

/// Theme files omarchy generates, ordered so the running terminal's own
/// file is tried first (the TUI then matches what the user actually sees).
fn theme_file_candidates() -> Vec<&'static str> {
    let mut files = vec![
        "kitty.conf",
        "ghostty.conf",
        "alacritty.toml",
        "foot.ini",
        "colors.toml",
    ];
    let term = std::env::var("TERM_PROGRAM")
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var("TERM").ok())
        .unwrap_or_default();
    let preferred = match term.as_str() {
        t if t.contains("ghostty") => "ghostty.conf",
        t if t.contains("kitty") => "kitty.conf",
        t if t.contains("alacritty") => "alacritty.toml",
        t if t.contains("foot") => "foot.ini",
        _ => "",
    };
    if let Some(position) = files.iter().position(|f| *f == preferred) {
        files.remove(position);
        files.insert(0, preferred);
    }
    files
}

/// Parse alacritty.toml `[colors.primary]` etc. (fallback when kitty.conf
/// is absent in the omarchy theme dir).
fn theme_from_alacritty(text: &str) -> Option<Theme> {
    let doc: toml::Value = toml::from_str(text).ok()?;
    let get = |path: &[&str]| -> Option<TColor> {
        let mut v = &doc;
        for p in path {
            v = v.get(p)?;
        }
        parse_hex(v.as_str()?)
    };
    let background = get(&["colors", "primary", "background"])?;
    let foreground = get(&["colors", "primary", "foreground"])
        .unwrap_or_else(|| contrast_foreground(background));
    let accent = get(&["colors", "normal", "blue"])
        .or_else(|| get(&["colors", "cursor", "cursor"]))
        .unwrap_or(TColor::Cyan);
    let selection = get(&["colors", "selection", "background"])
        .unwrap_or_else(|| blend(background, foreground, 0.18));
    let selection_foreground =
        get(&["colors", "selection", "text"]).unwrap_or_else(|| contrast_foreground(selection));
    let muted =
        get(&["colors", "bright", "black"]).unwrap_or_else(|| blend(background, foreground, 0.6));
    Some(Theme {
        background,
        foreground,
        accent,
        selection,
        selection_foreground,
        muted,
    })
}

/// Resolve the omarchy theme from disk, walking the fallback chain.
pub fn load_omarchy_theme() -> Option<Theme> {
    let dir = omarchy_theme_dir()?;
    for name in theme_file_candidates() {
        if let Ok(text) = std::fs::read_to_string(dir.join(name)) {
            if let Some(theme) = parse_theme_file(name, &text) {
                return Some(theme);
            }
        }
    }
    None
}

fn theme_from_colorfgbg(value: &str) -> Option<Theme> {
    let background = value.rsplit(';').next()?.parse::<u8>().ok()?;
    Some(Theme::builtin(if matches!(background, 7 | 15) {
        "light"
    } else {
        "nord"
    }))
}

/// Resolve a theme by config name.
pub fn resolve(name: &str) -> Theme {
    if name == "omarchy" {
        if let Some(t) = load_omarchy_theme() {
            return t;
        }
        if let Some(t) = std::env::var("COLORFGBG")
            .ok()
            .as_deref()
            .and_then(theme_from_colorfgbg)
        {
            return t;
        }
    }
    Theme::builtin(name)
}

/// Watches the resolved omarchy theme dir for file edits or replacement
/// (symlink retarget / directory swap). Non-blocking; poll with
/// [`ThemeWatcher::poll`].
pub struct ThemeWatcher {
    _watcher: notify::RecommendedWatcher,
    rx: std::sync::mpsc::Receiver<()>,
}

impl ThemeWatcher {
    pub fn new() -> Option<Self> {
        let dir = omarchy_theme_dir()?;
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |_res| {
            let _ = tx.send(());
        })
        .ok()?;
        use notify::Watcher as _;
        // Watch the parent `current` dir too so symlink retargets fire.
        let parent = dir.parent()?.to_path_buf();
        watcher
            .watch(&parent, notify::RecursiveMode::NonRecursive)
            .ok()?;
        watcher
            .watch(&dir, notify::RecursiveMode::NonRecursive)
            .ok()?;
        Some(Self {
            _watcher: watcher,
            rx,
        })
    }

    /// True when the theme changed since the last poll (drains the queue).
    pub fn poll(&self) -> bool {
        let mut changed = false;
        while self.rx.try_recv().is_ok() {
            changed = true;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kitty_conf_parses() {
        let text = "# comment\nbackground #1a1b26\nforeground #c0caf5\ncolor4 #7aa2f7\nselection_background #33467c\n";
        let t = theme_from_kitty(&parse_kitty_conf(text)).unwrap();
        assert_eq!(t.background, TColor::Rgb(0x1a, 0x1b, 0x26));
        assert_eq!(t.accent, TColor::Rgb(0x7a, 0xa2, 0xf7));
        assert!(t.is_dark());
    }

    #[test]
    fn ghostty_conf_parses() {
        let text = "background = #2e3440\nforeground = #d8dee9\ncursor-color = #d8dee9\nselection-background = #434c5e\nselection-foreground = #d8dee9\npalette = 4=#81a1c1\npalette = 8=#4c566a\n";
        let t = theme_from_ghostty(text).unwrap();
        assert_eq!(t.background, TColor::Rgb(0x2e, 0x34, 0x40));
        assert_eq!(t.foreground, TColor::Rgb(0xd8, 0xde, 0xe9));
        assert_eq!(t.selection, TColor::Rgb(0x43, 0x4c, 0x5e));
        assert_eq!(t.muted, TColor::Rgb(0x4c, 0x56, 0x6a));
        assert!(t.is_dark());
    }

    #[test]
    fn foot_ini_parses() {
        let text = "[colors-dark]\nforeground=d8dee9\nbackground=2e3440\nselection-foreground=d8dee9\nselection-background=434c5e\ncursor=2e3440 d8dee9\nregular4=81a1c1\nbright0=4c566a\n";
        let t = theme_from_foot(text).unwrap();
        assert_eq!(t.background, TColor::Rgb(0x2e, 0x34, 0x40));
        assert_eq!(t.foreground, TColor::Rgb(0xd8, 0xde, 0xe9));
        assert_eq!(t.accent, TColor::Rgb(0xd8, 0xde, 0xe9));
        assert_eq!(t.selection, TColor::Rgb(0x43, 0x4c, 0x5e));
        assert_eq!(t.muted, TColor::Rgb(0x4c, 0x56, 0x6a));
    }

    #[test]
    fn colors_toml_parses() {
        let text = "mode = \"dark\"\naccent = \"#81a1c1\"\nselection = \"#434c5e\"\nmuted = \"#4c566a\"\nbackground = \"#2e3440\"\nforeground = \"#d8dee9\"\n";
        let t = theme_from_colors_toml(text).unwrap();
        assert_eq!(t.background, TColor::Rgb(0x2e, 0x34, 0x40));
        assert_eq!(t.accent, TColor::Rgb(0x81, 0xa1, 0xc1));
        assert_eq!(t.selection, TColor::Rgb(0x43, 0x4c, 0x5e));
        assert_eq!(t.muted, TColor::Rgb(0x4c, 0x56, 0x6a));
    }

    #[test]
    fn light_colors_toml_gets_dark_text() {
        let text = "mode = \"light\"\nbackground = \"#ffffff\"\nforeground = \"#1f2328\"\n";
        let t = theme_from_colors_toml(text).unwrap();
        assert!(!t.is_dark());
        assert_eq!(t.foreground, TColor::Rgb(0x1f, 0x23, 0x28));
        assert_eq!(t.selection_foreground, TColor::Rgb(31, 35, 40));
    }

    #[test]
    fn alacritty_fallback_parses() {
        let text = "[colors.primary]\nbackground = \"#282828\"\nforeground = \"#ebdbb2\"\n[colors.normal]\nblue = \"#458588\"\n";
        let t = theme_from_alacritty(text).unwrap();
        assert_eq!(t.background, TColor::Rgb(0x28, 0x28, 0x28));
        assert_eq!(t.accent, TColor::Rgb(0x45, 0x85, 0x88));
    }

    #[test]
    fn unknown_theme_falls_back_to_builtin() {
        let t = resolve("no-such-theme");
        assert_eq!(t, Theme::builtin("nord-ish"));
    }

    #[test]
    fn builtin_themes_differ() {
        assert_ne!(Theme::builtin("dracula"), Theme::builtin("gruvbox"));
    }

    #[test]
    fn light_theme_uses_dark_text() {
        let theme = Theme::builtin("github_light");
        assert!(!theme.is_dark());
        assert_eq!(theme.foreground, TColor::Rgb(31, 35, 40));
        assert_eq!(theme.selection_foreground, TColor::Rgb(31, 35, 40));
    }

    #[test]
    fn light_kitty_theme_without_foreground_gets_dark_text() {
        let theme = theme_from_kitty(&parse_kitty_conf("background #ffffff\n")).unwrap();
        assert_eq!(theme.foreground, TColor::Rgb(31, 35, 40));
    }

    #[test]
    fn colorfgbg_detects_light_and_dark_backgrounds() {
        assert!(!theme_from_colorfgbg("0;15").unwrap().is_dark());
        assert!(theme_from_colorfgbg("15;0").unwrap().is_dark());
        assert!(theme_from_colorfgbg("invalid").is_none());
    }
}
