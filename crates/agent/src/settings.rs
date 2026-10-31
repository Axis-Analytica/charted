//! What the user chose: per user (`<state dir>/agent.json`), which agent and model, and the
//! providers they agreed to send the model's names to; per project
//! (`.cache/agent/settings.json`), whether the agent may see data.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserSettings {
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub models: BTreeMap<String, String>,
    /// Agents the user agreed to send to, with when (seconds since 1970).
    #[serde(default)]
    pub consent: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSettings {
    #[serde(default)]
    pub data: bool,
}

fn read<T: for<'a> Deserialize<'a> + Default>(path: &Path) -> T {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn write<T: Serialize>(path: &Path, v: &T) {
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(path, serde_json::to_vec_pretty(v).unwrap_or_default());
}

impl UserSettings {
    pub fn load(state_dir: &Path) -> Self {
        read(&state_dir.join("agent.json"))
    }
    pub fn save(&self, state_dir: &Path) {
        write(&state_dir.join("agent.json"), self)
    }
}

impl ProjectSettings {
    pub fn load(project: &Path) -> Self {
        read(&crate::transcript::dir(project).join("settings.json"))
    }
    pub fn save(&self, project: &Path) {
        write(&crate::transcript::dir(project).join("settings.json"), self)
    }
}

/// The project's instructions for the agent (naming rules, colours, "always use the Date
/// table"): `agent.md` in the project folder, kept with the project. The agent reads them
/// with every request.
pub const INSTRUCTIONS: &str = "agent.md";

/// The most of them sent with a request.
const INSTRUCTIONS_MAX: usize = 8000;

pub fn instructions(project: &Path) -> String {
    let text = std::fs::read_to_string(project.join(INSTRUCTIONS)).unwrap_or_default();
    match text.char_indices().nth(INSTRUCTIONS_MAX) {
        Some((at, _)) => text[..at].to_string(),
        None => text,
    }
}

/// Writes the instructions (removes the file when they're empty).
pub fn set_instructions(project: &Path, text: &str) -> Result<(), String> {
    let path = project.join(INSTRUCTIONS);
    if text.trim().is_empty() {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
    }
    let text = if text.ends_with('\n') {
        text.to_string()
    } else {
        format!("{text}\n")
    };
    std::fs::write(&path, text).map_err(|e| format!("couldn't write {}: {e}", path.display()))
}

/// Adds one instruction as a bullet at the end.
pub fn add_instruction(project: &Path, line: &str) -> Result<(), String> {
    let mut text = std::fs::read_to_string(project.join(INSTRUCTIONS)).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&format!("- {}\n", line.trim().trim_start_matches("- ")));
    set_instructions(project, &text)
}
