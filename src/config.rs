//! `~/.config/peepers/config.toml` — every key is optional.
//!
//! ```toml
//! accent    = "#CFAE74"   # one of the comp's accents, or any hex
//! pan_step  = 90          # px per h/j/k/l
//! prefetch  = 2           # neighbours decoded ahead on each side
//! cache_mb  = 768         # decoded-image budget
//! upscale   = false       # enlarge images smaller than the window
//! show_header = true      # brand + `?` shortcut bar (hidden in fullscreen)
//! show_hint = true        # "? motions  i info" on launch (header-less mode)
//!
//! [keys]                  # key = action  (use "none" to unbind)
//! "x"     = "quit"
//! "Right" = "next"
//! ```

use crate::motion::{default_keymap, Action};
use iced::Color;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct Raw {
    accent: Option<String>,
    pan_step: Option<f32>,
    prefetch: Option<usize>,
    cache_mb: Option<usize>,
    upscale: Option<bool>,
    show_hint: Option<bool>,
    show_header: Option<bool>,
    keys: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub accent: Color,
    pub pan_step: f32,
    pub prefetch: usize,
    pub cache_bytes: usize,
    pub upscale: bool,
    pub show_hint: bool,
    pub show_header: bool,
    pub keymap: HashMap<String, Action>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            accent: crate::theme::ACCENT,
            pan_step: 90.0,
            prefetch: 2,
            cache_bytes: 768 << 20,
            upscale: false,
            show_hint: true,
            show_header: true,
            keymap: default_keymap(),
        }
    }
}

pub fn path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("peepers").join("config.toml"))
}

pub fn load(explicit: Option<PathBuf>) -> Config {
    let mut cfg = Config::default();
    let Some(p) = explicit.or_else(path) else { return cfg };
    let Ok(text) = std::fs::read_to_string(&p) else { return cfg };
    let raw: Raw = match toml::from_str(&text) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("peepers: ignoring {}: {e}", p.display());
            return cfg;
        }
    };
    if let Some(c) = raw.accent.as_deref().and_then(parse_hex) {
        cfg.accent = c;
    }
    if let Some(v) = raw.pan_step {
        cfg.pan_step = v.clamp(1.0, 2000.0);
    }
    if let Some(v) = raw.prefetch {
        cfg.prefetch = v.min(8);
    }
    if let Some(v) = raw.cache_mb {
        cfg.cache_bytes = v.clamp(64, 16384) << 20;
    }
    if let Some(v) = raw.upscale {
        cfg.upscale = v;
    }
    if let Some(v) = raw.show_hint {
        cfg.show_hint = v;
    }
    if let Some(v) = raw.show_header {
        cfg.show_header = v;
    }
    for (key, name) in raw.keys {
        if name == "none" {
            cfg.keymap.remove(&key);
        } else if let Some(a) = Action::from_name(&name) {
            cfg.keymap.insert(key, a);
        } else {
            eprintln!("peepers: unknown action {name:?} for key {key:?}");
        }
    }
    cfg
}

pub fn parse_hex(s: &str) -> Option<Color> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(Color::from_rgb8((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_config_parses() {
        let raw: Raw = toml::from_str(include_str!("../extras/config.toml")).expect("example config");
        assert_eq!(raw.pan_step, Some(90.0));
    }

    #[test]
    fn rebinding() {
        let dir = std::env::temp_dir().join(format!("peepers-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("c.toml");
        std::fs::write(&p, "accent = \"#9FB8D0\"\n[keys]\nx = \"quit\"\nq = \"none\"\n").unwrap();
        let c = load(Some(p));
        assert_eq!(c.keymap.get("x"), Some(&Action::Quit));
        assert!(!c.keymap.contains_key("q"));
        assert_eq!(c.accent, parse_hex("#9FB8D0").unwrap());
    }
}
