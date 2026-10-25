use serde::{Deserialize, Serialize};
use std::process::Command;
use ts_rs::TS;

/// Window-manager settings that shape the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Desktop {
    /// Hyprland `decoration:rounding` in pixels; 0 means sharp corners.
    pub rounding: u32,
    /// Hyprland `animations:enabled`; when false the UI turns off its own motion.
    pub animations: bool,
    pub hyprland: bool,
}

fn hyprctl_option(name: &str) -> Option<serde_json::Value> {
    let out = Command::new("hyprctl")
        .args(["-j", "getoption", name])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

pub fn read_desktop() -> Desktop {
    let rounding = hyprctl_option("decoration:rounding");
    let animations = hyprctl_option("animations:enabled");
    Desktop {
        hyprland: rounding.is_some(),
        rounding: rounding
            .as_ref()
            .and_then(|v| v.get("int"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        animations: animations
            .as_ref()
            .and_then(|v| {
                v.get("bool")
                    .and_then(|b| b.as_bool())
                    .or_else(|| v.get("int").and_then(|i| i.as_i64()).map(|i| i != 0))
            })
            .unwrap_or(true),
    }
}
