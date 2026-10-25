use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::change::{Change, ChangeError, apply};
use crate::types::Model;

/// One undoable step: the changes as applied, and the changes that revert them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HistoryEntry {
    /// Human-readable label shown in Edit > Undo, e.g. "Rename column".
    pub label: String,
    pub changes: Vec<Change>,
    /// Inverse changes, already in the order they must be applied.
    pub inverse: Vec<Change>,
}

/// A linear undo/redo history with a saved marker for the unsaved-changes indicator.
#[derive(Debug, Clone, Default)]
pub struct History {
    entries: Vec<HistoryEntry>,
    /// Number of entries currently applied.
    cursor: usize,
    /// Cursor value at the last save, or `None` if that state is no longer reachable.
    saved: Option<usize>,
    limit: Option<usize>,
}

/// Applies `changes` as one transaction. On failure, everything applied so far is rolled
/// back and the model is unchanged.
pub fn apply_all(model: &mut Model, changes: &[Change]) -> Result<Vec<Change>, ChangeError> {
    let mut inverses: Vec<Vec<Change>> = Vec::with_capacity(changes.len());
    for change in changes {
        match apply(model, change.clone()) {
            Ok(inv) => inverses.push(inv),
            Err(e) => {
                for inv in inverses.into_iter().rev() {
                    for c in inv {
                        apply(model, c)
                            .expect("rolling back a change that just applied cannot fail");
                    }
                }
                return Err(e);
            }
        }
    }
    Ok(inverses.into_iter().rev().flatten().collect())
}

impl History {
    pub fn new() -> Self {
        Self {
            saved: Some(0),
            ..Self::default()
        }
    }

    /// Keeps at most `limit` entries; the oldest are dropped first.
    pub fn with_limit(limit: usize) -> Self {
        Self {
            limit: Some(limit),
            ..Self::new()
        }
    }

    /// Applies a transaction and records it, discarding any redoable entries.
    pub fn commit(
        &mut self,
        model: &mut Model,
        label: impl Into<String>,
        changes: Vec<Change>,
    ) -> Result<(), ChangeError> {
        if changes.is_empty() {
            return Ok(());
        }
        let inverse = apply_all(model, &changes)?;
        self.entries.truncate(self.cursor);
        if self.saved.is_some_and(|s| s > self.cursor) {
            self.saved = None;
        }
        self.entries.push(HistoryEntry {
            label: label.into(),
            changes,
            inverse,
        });
        self.cursor += 1;
        if let Some(limit) = self.limit {
            while self.entries.len() > limit {
                self.entries.remove(0);
                self.cursor -= 1;
                self.saved = self.saved.and_then(|s| s.checked_sub(1));
            }
        }
        Ok(())
    }

    /// Reverts the latest entry. Returns its label, or `None` when there is nothing to undo.
    pub fn undo(&mut self, model: &mut Model) -> Result<Option<String>, ChangeError> {
        if self.cursor == 0 {
            return Ok(None);
        }
        let entry = &self.entries[self.cursor - 1];
        apply_all(model, &entry.inverse)?;
        self.cursor -= 1;
        Ok(Some(entry.label.clone()))
    }

    /// Re-applies the next entry. Returns its label, or `None` when there is nothing to redo.
    pub fn redo(&mut self, model: &mut Model) -> Result<Option<String>, ChangeError> {
        let Some(entry) = self.entries.get(self.cursor) else {
            return Ok(None);
        };
        let inverse = apply_all(model, &entry.changes)?;
        self.entries[self.cursor].inverse = inverse;
        let label = self.entries[self.cursor].label.clone();
        self.cursor += 1;
        Ok(Some(label))
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.cursor
            .checked_sub(1)
            .map(|i| self.entries[i].label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.entries.get(self.cursor).map(|e| e.label.as_str())
    }

    /// Drops every entry: what was done can no longer be undone (the data underneath
    /// changed, as after applying query edits). The model counts as unsaved.
    pub fn forget(&mut self) {
        self.entries.clear();
        self.cursor = 0;
        self.saved = None;
    }

    pub fn mark_saved(&mut self) {
        self.saved = Some(self.cursor);
    }

    pub fn is_dirty(&self) -> bool {
        self.saved != Some(self.cursor)
    }

    /// Entries applied since the last save, oldest first (used by the autosave journal).
    pub fn unsaved_entries(&self) -> &[HistoryEntry] {
        let from = self.saved.unwrap_or(0).min(self.cursor);
        &self.entries[from..self.cursor]
    }
}
