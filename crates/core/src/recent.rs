//! The recently opened projects list, kept in the user's state directory.

use serde::{Deserialize, Serialize};
use std::path::Path;
use ts_rs::TS;

use crate::atomic;
use crate::session::now;

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecentProject {
    pub name: String,
    pub path: String,
    #[ts(type = "number")]
    pub opened_at: u64,
    /// False when the project has since been moved or deleted.
    #[serde(default)]
    pub exists: bool,
}

const LIMIT: usize = 12;

fn file(state_dir: &Path) -> std::path::PathBuf {
    state_dir.join("recent.json")
}

fn read(state_dir: &Path) -> Vec<RecentProject> {
    std::fs::read_to_string(file(state_dir))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn write(state_dir: &Path, items: &[RecentProject]) {
    if let Ok(json) = serde_json::to_vec_pretty(items) {
        let _ = atomic::write(&file(state_dir), &json);
    }
}

pub fn list(state_dir: &Path) -> Vec<RecentProject> {
    read(state_dir)
        .into_iter()
        .map(|mut r| {
            r.exists = Path::new(&r.path).exists();
            r
        })
        .collect()
}

pub fn remember(state_dir: &Path, name: &str, manifest: &Path) {
    let path = manifest.display().to_string();
    let mut items = read(state_dir);
    items.retain(|r| r.path != path);
    items.insert(
        0,
        RecentProject {
            name: name.into(),
            path,
            opened_at: now(),
            exists: true,
        },
    );
    items.truncate(LIMIT);
    write(state_dir, &items);
}

pub fn forget(state_dir: &Path, manifest: &Path) {
    let path = manifest.display().to_string();
    let mut items = read(state_dir);
    items.retain(|r| r.path != path);
    write(state_dir, &items);
}
