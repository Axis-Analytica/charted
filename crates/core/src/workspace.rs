//! How charted itself is laid out, per user: named profiles (Standard, Report + model,
//! Two report pages, and the user's own), each a [`Layout`] of where the panes go (docked
//! left or right, floating, or hidden), an optional second view beside the first, and the
//! commands hidden from toolbars and menus. Kept in `<state dir>/workspace.json`, with its
//! own undo (layout isn't the project's), and changed by the user or their agent.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Place {
    Left,
    Right,
    Float,
    Hidden,
}

/// Where a floating pane is, in pixels from the top left of the report area.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FloatRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PaneLayout {
    pub place: Place,
    /// A docked pane's width, when the user resized it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub width: Option<f64>,
    /// Where it floats (kept while docked, so floating again puts it back).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub float: Option<FloatRect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ViewKind {
    Report,
    Data,
    Model,
    Query,
}

/// A second view beside the one picked in the rail; a second report shows its own page.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Split {
    pub view: ViewKind,
    /// The first view's share of the width (0.2–0.8).
    pub ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Layout {
    /// By pane: bookmarks, selection, sync, filters, visualizations, fields, agent. A pane
    /// not listed takes its default place.
    #[serde(default)]
    pub panes: BTreeMap<String, PaneLayout>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub split: Option<Split>,
    /// Commands left out of toolbars and menus (still in the command palette).
    #[serde(default)]
    pub hidden: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Profile {
    pub id: String,
    pub name: String,
    /// The built-in profile it started as (it can be reset to it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub builtin: Option<String>,
    pub layout: Layout,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Workspace {
    pub profiles: Vec<Profile>,
    /// The profile in use, by id.
    pub active: String,
}

/// A command the user can hide, as the UI lists it (for the agent's `list_commands`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CommandInfo {
    pub id: String,
    pub label: String,
    /// Where it shows: "report toolbar", "file menu", "top bar".
    pub place: String,
    /// How often the user has used it on this machine.
    pub uses: u32,
}

/// The panes, in the order they dock.
pub const PANES: &[&str] = &[
    "bookmarks",
    "selection",
    "sync",
    "filters",
    "visualizations",
    "fields",
    "agent",
];

/// A pane's place when the layout doesn't say.
pub fn default_place(pane: &str) -> Place {
    match pane {
        "filters" | "visualizations" | "fields" => Place::Right,
        _ => Place::Hidden,
    }
}

/// The built-in profiles: (id, name, layout).
pub fn builtins() -> Vec<(&'static str, &'static str, Layout)> {
    let split = |view| Some(Split { view, ratio: 0.5 });
    vec![
        ("standard", "Standard", Layout::default()),
        (
            "report-model",
            "Report + model",
            Layout {
                split: split(ViewKind::Model),
                ..Layout::default()
            },
        ),
        (
            "two-reports",
            "Two report pages",
            Layout {
                split: split(ViewKind::Report),
                ..Layout::default()
            },
        ),
    ]
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            profiles: builtins()
                .into_iter()
                .map(|(id, name, layout)| Profile {
                    id: id.into(),
                    name: name.into(),
                    builtin: Some(id.into()),
                    layout,
                })
                .collect(),
            active: "standard".into(),
        }
    }
}

const FILE: &str = "workspace.json";

impl Workspace {
    pub fn read(state_dir: &Path) -> Self {
        let mut w: Workspace = std::fs::read(state_dir.join(FILE))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        if w.profiles.is_empty() {
            w = Workspace::default();
        }
        if !w.profiles.iter().any(|p| p.id == w.active) {
            w.active = w.profiles[0].id.clone();
        }
        w
    }

    pub fn write(&self, state_dir: &Path) {
        let _ = std::fs::create_dir_all(state_dir);
        let tmp = state_dir.join(format!("{FILE}.part"));
        if std::fs::write(&tmp, serde_json::to_vec_pretty(self).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(tmp, state_dir.join(FILE));
        }
    }

    pub fn active(&self) -> &Profile {
        self.profiles
            .iter()
            .find(|p| p.id == self.active)
            .unwrap_or(&self.profiles[0])
    }

    fn active_mut(&mut self) -> &mut Profile {
        let i = self
            .profiles
            .iter()
            .position(|p| p.id == self.active)
            .unwrap_or(0);
        &mut self.profiles[i]
    }

    /// A profile by id or (any case) name.
    pub fn find(&self, key: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == key).or_else(|| {
            self.profiles
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case(key.trim()))
        })
    }
}

/// A layout as it is used: values in range, unknown panes dropped.
pub fn clean(mut l: Layout) -> Layout {
    l.panes.retain(|k, _| PANES.contains(&k.as_str()));
    for p in l.panes.values_mut() {
        p.width = p.width.map(|w| w.clamp(180.0, 640.0));
        if let Some(f) = &mut p.float {
            f.width = f.width.clamp(180.0, 1600.0);
            f.height = f.height.clamp(120.0, 1400.0);
            f.x = f.x.max(0.0);
            f.y = f.y.max(0.0);
        }
    }
    if let Some(s) = &mut l.split {
        s.ratio = if s.ratio.is_finite() {
            s.ratio.clamp(0.2, 0.8)
        } else {
            0.5
        };
    }
    l.hidden.sort();
    l.hidden.dedup();
    l
}

/// The workspace and its undo: each entry is the active profile's layout before a change,
/// with the agent request it belongs to (a request's changes undo as one).
#[derive(Debug, Clone, Default)]
pub struct Layouts {
    pub workspace: Workspace,
    undo: Vec<(String, Layout, Option<String>)>,
    redo: Vec<(String, Layout)>,
}

impl Layouts {
    pub fn read(state_dir: &Path) -> Self {
        Self {
            workspace: Workspace::read(state_dir),
            ..Self::default()
        }
    }

    pub fn can_undo(&self) -> bool {
        self.undo
            .last()
            .is_some_and(|(p, _, _)| *p == self.workspace.active)
    }

    pub fn can_redo(&self) -> bool {
        self.redo
            .last()
            .is_some_and(|(p, _)| *p == self.workspace.active)
    }

    /// Replaces the active profile's layout. `group`: the agent request it is part of.
    pub fn set(&mut self, layout: Layout, group: Option<&str>) {
        let layout = clean(layout);
        let active = self.workspace.active.clone();
        let p = self.workspace.active_mut();
        if p.layout == layout {
            return;
        }
        let old = std::mem::replace(&mut p.layout, layout);
        let joins = group.is_some()
            && self
                .undo
                .last()
                .is_some_and(|(id, _, g)| *id == active && g.as_deref() == group);
        if !joins {
            self.undo.push((active, old, group.map(str::to_string)));
            if self.undo.len() > 100 {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
    }

    pub fn undo(&mut self) -> bool {
        if !self.can_undo() {
            return false;
        }
        let (id, layout, _) = self.undo.pop().expect("checked");
        let p = self.workspace.active_mut();
        self.redo
            .push((id, std::mem::replace(&mut p.layout, layout)));
        true
    }

    pub fn redo(&mut self) -> bool {
        if !self.can_redo() {
            return false;
        }
        let (id, layout) = self.redo.pop().expect("checked");
        let p = self.workspace.active_mut();
        self.undo
            .push((id, std::mem::replace(&mut p.layout, layout), None));
        true
    }

    pub fn switch(&mut self, key: &str) -> Result<(), String> {
        let id = self
            .workspace
            .find(key)
            .ok_or_else(|| format!("there's no profile \"{key}\""))?
            .id
            .clone();
        self.workspace.active = id;
        Ok(())
    }

    /// A new profile, a copy of `from` (else the active one), made active.
    pub fn add(&mut self, name: &str, from: Option<&str>) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("a profile needs a name".into());
        }
        if self
            .workspace
            .profiles
            .iter()
            .any(|p| p.name.eq_ignore_ascii_case(name))
        {
            return Err(format!("there's a profile \"{name}\" already"));
        }
        let layout = match from {
            Some(k) => self
                .workspace
                .find(k)
                .ok_or_else(|| format!("there's no profile \"{k}\""))?
                .layout
                .clone(),
            None => self.workspace.active().layout.clone(),
        };
        let id = uuid::Uuid::new_v4().to_string();
        self.workspace.profiles.push(Profile {
            id: id.clone(),
            name: name.into(),
            builtin: None,
            layout,
        });
        self.workspace.active = id.clone();
        Ok(id)
    }

    pub fn rename(&mut self, key: &str, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("a profile needs a name".into());
        }
        let id = self
            .workspace
            .find(key)
            .ok_or_else(|| format!("there's no profile \"{key}\""))?
            .id
            .clone();
        if self
            .workspace
            .profiles
            .iter()
            .any(|p| p.id != id && p.name.eq_ignore_ascii_case(name))
        {
            return Err(format!("there's a profile \"{name}\" already"));
        }
        if let Some(p) = self.workspace.profiles.iter_mut().find(|p| p.id == id) {
            p.name = name.into();
        }
        Ok(())
    }

    pub fn delete(&mut self, key: &str) -> Result<(), String> {
        if self.workspace.profiles.len() <= 1 {
            return Err("there must be at least one profile".into());
        }
        let id = self
            .workspace
            .find(key)
            .ok_or_else(|| format!("there's no profile \"{key}\""))?
            .id
            .clone();
        self.workspace.profiles.retain(|p| p.id != id);
        self.undo.retain(|(p, _, _)| *p != id);
        self.redo.retain(|(p, _)| *p != id);
        if self.workspace.active == id {
            self.workspace.active = self.workspace.profiles[0].id.clone();
        }
        Ok(())
    }

    /// Puts a built-in profile back as it ships (an undoable layout change).
    pub fn reset(&mut self, key: &str) -> Result<(), String> {
        let p = self
            .workspace
            .find(key)
            .ok_or_else(|| format!("there's no profile \"{key}\""))?;
        let Some(b) = p.builtin.clone() else {
            return Err(format!("\"{}\" isn't a built-in profile", p.name));
        };
        let id = p.id.clone();
        let layout = builtins()
            .into_iter()
            .find(|(i, _, _)| *i == b)
            .map(|(_, _, l)| l)
            .unwrap_or_default();
        self.workspace.active = id;
        self.set(layout, None);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floated(pane: &str) -> Layout {
        let mut l = Layout::default();
        l.panes.insert(
            pane.into(),
            PaneLayout {
                place: Place::Float,
                width: None,
                float: Some(FloatRect {
                    x: 10.0,
                    y: 10.0,
                    width: 300.0,
                    height: 400.0,
                }),
            },
        );
        l
    }

    #[test]
    fn layouts_undo_and_a_request_undoes_as_one() {
        let mut l = Layouts::default();
        assert!(!l.can_undo());
        l.set(floated("fields"), None);
        assert!(l.can_undo());
        l.set(floated("filters"), Some("g"));
        let mut both = floated("filters");
        both.hidden = vec!["align-left".into()];
        l.set(both.clone(), Some("g"));
        assert_eq!(l.workspace.active().layout, both);
        // The agent's two changes undo together, back to the user's.
        assert!(l.undo());
        assert_eq!(l.workspace.active().layout, floated("fields"));
        assert!(l.redo());
        assert_eq!(l.workspace.active().layout, both);
    }

    #[test]
    fn profiles_switch_copy_rename_reset_and_delete() {
        let mut l = Layouts::default();
        assert_eq!(l.workspace.profiles.len(), 3);
        l.switch("Report + model").unwrap();
        assert_eq!(
            l.workspace.active().layout.split.unwrap().view,
            ViewKind::Model
        );
        let id = l.add("Mine", None).unwrap();
        assert_eq!(l.workspace.active, id);
        assert!(l.add("mine", None).is_err());
        l.rename("Mine", "Wide").unwrap();
        l.switch("two-reports").unwrap();
        l.set(floated("fields"), None);
        l.reset("Two report pages").unwrap();
        assert_eq!(
            l.workspace.active().layout.split.unwrap().view,
            ViewKind::Report
        );
        assert!(l.workspace.active().layout.panes.is_empty());
        l.delete("Wide").unwrap();
        assert_eq!(l.workspace.profiles.len(), 3);
        assert!(l.reset("Nope").is_err());
    }

    #[test]
    fn layouts_are_kept_in_range() {
        let mut l = floated("fields");
        l.panes.get_mut("fields").unwrap().width = Some(5.0);
        l.panes.insert("nonsense".into(), l.panes["fields"].clone());
        l.split = Some(Split {
            view: ViewKind::Report,
            ratio: 3.0,
        });
        let c = clean(l);
        assert_eq!(c.panes["fields"].width, Some(180.0));
        assert!(!c.panes.contains_key("nonsense"));
        assert_eq!(c.split.unwrap().ratio, 0.8);
    }

    #[test]
    fn a_missing_or_broken_file_gives_the_built_ins() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(Workspace::read(d.path()).profiles.len(), 3);
        std::fs::write(d.path().join(FILE), "{nope").unwrap();
        assert_eq!(Workspace::read(d.path()).active, "standard");
        let mut l = Layouts::read(d.path());
        l.add("Mine", None).unwrap();
        l.workspace.write(d.path());
        assert_eq!(Workspace::read(d.path()).profiles.len(), 4);
    }
}
