//! Desktop integration with Omarchy and Hyprland: theme colours, window rounding,
//! animations and language, each read from the same places Omarchy's own apps use, and
//! watched so the app follows changes live.

pub mod desktop;
pub mod language;
pub mod theme;
pub mod watch;

pub use desktop::{Desktop, read_desktop};
pub use language::{Language, read_language};
pub use theme::{Theme, read_theme};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The complete desktop environment, sent to the UI at startup and on every change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EnvSnapshot {
    pub theme: Theme,
    pub desktop: Desktop,
    pub language: Language,
    /// The Omarchy monospace font family, used for code and formulas.
    pub mono_font: Option<String>,
}

impl EnvSnapshot {
    pub fn read(paths: &watch::EnvPaths) -> Self {
        Self {
            theme: paths.theme(),
            desktop: watch::desktop_now(),
            language: paths.language(),
            mono_font: mono_font(),
        }
    }
}

fn mono_font() -> Option<String> {
    let out = std::process::Command::new("omarchy-font-current")
        .output()
        .ok()?;
    let name = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!name.is_empty()).then_some(name)
}
