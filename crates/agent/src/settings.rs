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
