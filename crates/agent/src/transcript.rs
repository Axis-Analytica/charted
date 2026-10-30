//! The Agent pane's conversation, kept with the project (`.cache/agent/chat.json`, which
//! bundles and exports leave out) so follow-ups work after reopening it.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use ts_rs::TS;

use crate::tools::Step;

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Transcript {
    /// The agent's own session for each agent (claude, codex), so a follow-up continues it.
    #[serde(default)]
    pub sessions: BTreeMap<String, String>,
    #[serde(default)]
    pub turns: Vec<TurnRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum TurnStatus {
    Running,
    Done,
    Stopped,
    Failed,
}

/// One request and what came of it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TurnRecord {
    pub id: String,
    /// The undo group of its changes.
    pub group: String,
    pub request: String,
    /// What the user was looking at ("Page 1 · 1 visual selected").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub context: Option<String>,
    pub agent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub model: Option<String>,
    pub status: TurnStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cost_usd: Option<f64>,
    #[serde(default)]
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum Item {
    /// What the agent said.
    Text { text: String },
    /// A tool it used that changed something, or failed.
    Step { step: Step },
    /// Something went wrong with the agent itself.
    Error { message: String },
    /// The agent asks to delete something; `answer` once the user has.
    Confirm {
        id: String,
        what: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        answer: Option<bool>,
    },
}

pub fn dir(project: &Path) -> PathBuf {
    project.join(".cache").join("agent")
}

impl Transcript {
    pub fn load(project: &Path) -> Self {
        let mut t: Transcript = std::fs::read(dir(project).join("chat.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        // A turn that was running when charted closed ended there.
        for turn in &mut t.turns {
            if turn.status == TurnStatus::Running {
                turn.status = TurnStatus::Stopped;
            }
        }
        t
    }

    pub fn save(&self, project: &Path) {
        let d = dir(project);
        if std::fs::create_dir_all(&d).is_ok() {
            let tmp = d.join("chat.json.part");
            if std::fs::write(&tmp, serde_json::to_vec(self).unwrap_or_default()).is_ok() {
                let _ = std::fs::rename(tmp, d.join("chat.json"));
            }
        }
    }
}
