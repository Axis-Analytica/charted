use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use ts_rs::TS;

/// Omarchy's accent fallback, used when not running on Omarchy.
pub const DEFAULT_ACCENT: &str = "#FFD60A";

/// The desktop theme's palette. `colors` holds every key from `colors.toml` (accent,
/// background, foreground, red, …); the UI derives its design tokens from it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Theme {
    pub name: Option<String>,
    /// "dark" or "light".
    pub mode: String,
    pub colors: BTreeMap<String, String>,
    /// False when charted is not running on Omarchy and uses its built-in palette.
    pub from_omarchy: bool,
}

pub fn theme_dir(home: &Path) -> PathBuf {
    home.join(".local/state/omarchy/current/theme")
}

pub fn fallback() -> Theme {
    let colors = [
        ("accent", DEFAULT_ACCENT),
        ("background", "#121212"),
        ("foreground", "#e8e8e8"),
        ("selection", "#2a2a2a"),
        ("muted", "#3a3a3a"),
        ("red", "#ef5f5f"),
        ("yellow", "#FFD60A"),
        ("green", "#5fd38d"),
        ("cyan", "#4fd6d6"),
        ("blue", "#6aa6ff"),
        ("magenta", "#c78cff"),
        ("orange", "#ff9f43"),
    ];
    Theme {
        name: None,
        mode: "dark".into(),
        colors: colors
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        from_omarchy: false,
    }
}

/// Reads the current Omarchy theme from `dir` (normally `theme_dir(home)`), falling back
/// to the built-in palette when it is missing or unreadable.
pub fn read_theme(dir: &Path) -> Theme {
    let Ok(text) = std::fs::read_to_string(dir.join("colors.toml")) else {
        return fallback();
    };
    let Ok(table) = text.parse::<toml::Table>() else {
        return fallback();
    };
    let mut colors = BTreeMap::new();
    let mut mode = None;
    for (k, v) in table {
        if let toml::Value::String(s) = v {
            if k == "mode" {
                mode = Some(s);
            } else if s.starts_with('#') {
                colors.insert(k, s);
            }
        }
    }
    if !colors.contains_key("background") || !colors.contains_key("foreground") {
        return fallback();
    }
    colors
        .entry("accent".into())
        .or_insert_with(|| DEFAULT_ACCENT.into());
    let mode = mode.unwrap_or_else(|| {
        if is_light(&colors["background"]) {
            "light".into()
        } else {
            "dark".into()
        }
    });
    let name = dir
        .parent()
        .and_then(|p| std::fs::read_to_string(p.join("theme.name")).ok())
        .map(|s| s.trim().to_string());
    Theme {
        name,
        mode,
        colors,
        from_omarchy: true,
    }
}

fn is_light(hex: &str) -> bool {
    let h = hex.trim_start_matches('#');
    let c = |i: usize| {
        u8::from_str_radix(h.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0) as f64 / 255.0
    };
    0.2126 * c(0) + 0.7152 * c(2) + 0.0722 * c(4) > 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_colors_and_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!read_theme(dir.path()).from_omarchy);
        std::fs::write(dir.path().join("colors.toml"), "accent = \"#f97316\"\nbackground = \"#0a0a0f\"\nforeground = \"#e4e4e7\"\nbrown = \"#9a3412\"\n").unwrap();
        let t = read_theme(dir.path());
        assert!(t.from_omarchy);
        assert_eq!(t.mode, "dark", "inferred from background luminance");
        assert_eq!(t.colors["brown"], "#9a3412", "unknown keys are kept");
        std::fs::write(
            dir.path().join("colors.toml"),
            "mode = \"light\"\nbackground = \"#fafafa\"\nforeground = \"#111111\"\n",
        )
        .unwrap();
        let t = read_theme(dir.path());
        assert_eq!(t.colors["accent"], DEFAULT_ACCENT);
        assert_eq!(t.mode, "light");
    }
}
