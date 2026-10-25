//! The report built on the model: pages of positioned visuals, each binding model fields
//! to the visual's data roles, plus filters at report, page and visual level.
//!
//! Field references use model ids, so renaming a column or measure never breaks a
//! visual. Report files (`report/…`) are written by charted-core; the Ossie document
//! never contains the report.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::types::Id;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Report {
    #[serde(default)]
    pub pages: Vec<Page>,
    /// Filters on all pages.
    #[serde(default)]
    pub filters: Vec<ReportFilter>,
    /// Where each table's card sits in the model view, by table id. Tables without a
    /// position are laid out automatically.
    #[serde(default)]
    pub diagram: std::collections::BTreeMap<Id, DiagramPosition>,
    /// The report theme; `None` follows the Omarchy desktop theme live.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<ReportTheme>,
    /// Saved views of the report, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bookmarks: Vec<Bookmark>,
}

/// A saved view: a page with the report's and the page's filters, and each visual's
/// filters, slicer choice, sort and drill state, as they were.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Bookmark {
    pub id: Id,
    pub name: String,
    pub page: Id,
    #[serde(default)]
    pub report_filters: Vec<ReportFilter>,
    #[serde(default)]
    pub page_filters: Vec<ReportFilter>,
    /// The state of each visual on the page, by visual id.
    #[serde(default)]
    pub visuals: std::collections::BTreeMap<Id, VisualState>,
}

/// The parts of a visual a bookmark keeps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VisualState {
    #[serde(default)]
    pub filters: Vec<ReportFilter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<FilterKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<VisualSort>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drill: Vec<DrillStep>,
}

/// A report theme in Power BI's theme JSON format (`name`, `dataColors`, `background`,
/// `foreground`, `tableAccent`, …), kept whole so nothing in an imported file is lost.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReportTheme {
    pub name: String,
    #[ts(type = "Record<string, unknown>")]
    pub json: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DiagramPosition {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Page {
    pub id: Id,
    #[serde(flatten)]
    pub settings: PageSettings,
    #[serde(default)]
    pub visuals: Vec<Visual>,
}

/// Everything about a page except its visuals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PageSettings {
    pub name: String,
    #[serde(default = "default_width")]
    pub width: u32,
    #[serde(default = "default_height")]
    pub height: u32,
    /// Filters on this page.
    #[serde(default)]
    pub filters: Vec<ReportFilter>,
    /// How selecting in one visual affects another, where it differs from the default.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub interactions: Vec<Interaction>,
    /// Drill-through fields: other pages' data points with these fields can drill
    /// through to this page, which then shows them filtered to the point.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drillthrough: Vec<Projection>,
    /// Drilling through also brings the source visual's other filters (its page's
    /// filters and slicers, and the point's other fields).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub keep_all_filters: bool,
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

/// What a selection in `source` does to `target`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Interaction {
    pub source: Id,
    pub target: Id,
    pub kind: InteractionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum InteractionKind {
    Filter,
    Highlight,
    None,
}

fn default_width() -> u32 {
    1280
}

fn default_height() -> u32 {
    720
}

impl PageSettings {
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            width: default_width(),
            height: default_height(),
            filters: Vec::new(),
            interactions: Vec::new(),
            drillthrough: Vec::new(),
            keep_all_filters: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Position {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    #[serde(default)]
    pub z: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Visual {
    pub id: Id,
    /// The visual type, e.g. "card", "columnChart", "barChart", "lineChart", "table".
    pub visual_type: String,
    pub position: Position,
    /// Fields bound to each data role, in order.
    #[serde(default)]
    pub roles: std::collections::BTreeMap<String, Vec<Projection>>,
    #[serde(default)]
    pub filters: Vec<ReportFilter>,
    /// The visual's own sort: a projection (role and index) and direction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<VisualSort>,
    /// Formatting properties by object, e.g. `{"title": {"show": true}}`.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    #[ts(type = "Record<string, Record<string, unknown>>")]
    pub objects: serde_json::Map<String, serde_json::Value>,
    /// A slicer's current choice, which filters the other visuals on its page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<FilterKind>,
    /// How far the visual is drilled down its hierarchy (the fields of its axis or
    /// legend), one step per level below the top.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drill: Vec<DrillStep>,
}

/// One level of drilling down a visual's hierarchy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum DrillStep {
    /// Into one member: the next level, filtered to it. `keys` are the member's values
    /// (as text, none for blank) at the levels shown, the last at this level.
    Into { keys: Vec<Option<String>> },
    /// Go to the next level: the next level for every member.
    Next,
    /// Expand all down one level: this level and the next, together.
    Expand,
}

/// The role whose fields form a visual type's drillable hierarchy.
pub fn drill_role(visual_type: &str) -> Option<&'static str> {
    match visual_type {
        "columnChart" | "barChart" | "stackedColumnChart" | "stackedBarChart" | "lineChart" => {
            Some("category")
        }
        "pieChart" | "donutChart" => Some("legend"),
        _ => None,
    }
}

impl Visual {
    /// The levels of the drill hierarchy shown (`start..=depth`), and the members
    /// drilled into above them (level, key). `levels` is the hierarchy's field count.
    pub fn drill_levels(&self, levels: usize) -> (usize, usize, Vec<(usize, Option<String>)>) {
        let mut start = 0;
        let mut into = Vec::new();
        let steps = &self.drill[..self.drill.len().min(levels.saturating_sub(1))];
        for (i, s) in steps.iter().enumerate() {
            match s {
                DrillStep::Into { keys } => {
                    let first = (i + 1).saturating_sub(keys.len());
                    for (j, k) in keys.iter().enumerate() {
                        into.push((first + j, k.clone()));
                    }
                    start = i + 1;
                }
                DrillStep::Next => start = i + 1,
                DrillStep::Expand => {}
            }
        }
        (start, steps.len(), into)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VisualSort {
    pub role: String,
    pub index: usize,
    #[serde(default)]
    pub descending: bool,
}

/// A model field: a column or a measure, by id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum FieldRef {
    Column { table: Id, column: Id },
    Measure { table: Id, measure: Id },
}

/// How a column is summarised when used as a value (an implicit measure).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Aggregation {
    Sum,
    Average,
    Min,
    Max,
    Count,
    DistinctCount,
}

/// A field placed in a role.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Projection {
    pub field: FieldRef,
    /// For a column in a value role: how it is summarised. `None` groups by it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregation: Option<Aggregation>,
    /// Overrides the field name in this visual.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FilterOp {
    Equals,
    NotEquals,
    Contains,
    NotContains,
    StartsWith,
    EndsWith,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
    Between,
    IsBlank,
    IsNotBlank,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Condition {
    pub op: FilterOp,
    #[serde(default)]
    pub value: Option<String>,
    /// The upper bound of `between`.
    #[serde(default)]
    pub value2: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum FilterKind {
    /// Not filtering yet (a field added to the filters pane without a choice).
    All,
    /// Keeps rows whose value, as text, is listed (or with `exclude`, is not). `null` is
    /// blank.
    Basic {
        values: Vec<Option<String>>,
        #[serde(default)]
        exclude: bool,
    },
    /// One or two conditions joined by and/or.
    Advanced {
        conditions: Vec<Condition>,
        #[serde(default)]
        or: bool,
    },
    /// Keeps the top (or bottom) `count` values of the field, ranked by `by`.
    TopN {
        count: u32,
        #[serde(default)]
        bottom: bool,
        by: Projection,
    },
    /// Dates relative to today: the last or next `count` units (today included), or the
    /// current unit (`this`, `count` ignored).
    RelativeDate {
        period: RelativePeriod,
        count: u32,
        unit: DateUnit,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum RelativePeriod {
    Last,
    This,
    Next,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum DateUnit {
    Days,
    Weeks,
    Months,
    Years,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReportFilter {
    pub id: Id,
    pub field: Projection,
    pub filter: FilterKind,
}

impl ReportFilter {
    /// Whether the filter restricts anything.
    pub fn is_active(&self) -> bool {
        !matches!(self.filter, FilterKind::All)
    }
}

impl Report {
    pub fn page(&self, id: &str) -> Option<&Page> {
        self.pages.iter().find(|p| p.id == id)
    }

    pub fn page_mut(&mut self, id: &str) -> Option<&mut Page> {
        self.pages.iter_mut().find(|p| p.id == id)
    }

    /// A visual anywhere in the report, with its page.
    pub fn visual(&self, id: &str) -> Option<(&Page, &Visual)> {
        self.pages
            .iter()
            .find_map(|p| p.visuals.iter().find(|v| v.id == id).map(|v| (p, v)))
    }
}

impl Page {
    pub fn visual(&self, id: &str) -> Option<&Visual> {
        self.visuals.iter().find(|v| v.id == id)
    }

    /// What a selection in `source` does to `target`: the page's setting, or the
    /// default for the target's type (charts that can show a highlight highlight; the
    /// rest filter).
    pub fn interaction(&self, source: &str, target: &str) -> InteractionKind {
        if let Some(i) = self
            .settings
            .interactions
            .iter()
            .find(|i| i.source == source && i.target == target)
        {
            return i.kind;
        }
        match self.visual(target) {
            Some(v) if v.can_highlight() => InteractionKind::Highlight,
            _ => InteractionKind::Filter,
        }
    }
}

impl Visual {
    /// Whether the visual can show a selection elsewhere as a highlight (part of each
    /// bar or slice) rather than only as a filter.
    pub fn can_highlight(&self) -> bool {
        matches!(
            self.visual_type.as_str(),
            "columnChart"
                | "barChart"
                | "stackedColumnChart"
                | "stackedBarChart"
                | "pieChart"
                | "donutChart"
                | "scatterChart"
        )
    }

    pub fn is_slicer(&self) -> bool {
        self.visual_type == "slicer"
    }

    /// Every field the visual uses, in role order.
    pub fn projections(&self) -> impl Iterator<Item = (&str, &Projection)> {
        self.roles
            .iter()
            .flat_map(|(r, ps)| ps.iter().map(move |p| (r.as_str(), p)))
    }
}

#[cfg(test)]
mod drill_tests {
    use super::*;

    fn visual(drill: Vec<DrillStep>) -> Visual {
        Visual {
            id: "v".into(),
            visual_type: "columnChart".into(),
            position: Position {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
                z: 0,
            },
            roles: Default::default(),
            filters: vec![],
            sort: None,
            objects: Default::default(),
            selection: None,
            drill,
        }
    }

    #[test]
    fn drill_levels_follow_the_steps() {
        let k = |s: &str| Some(s.to_string());
        assert_eq!(visual(vec![]).drill_levels(3), (0, 0, vec![]));
        assert_eq!(
            visual(vec![DrillStep::Into {
                keys: vec![k("2024")]
            }])
            .drill_levels(3),
            (1, 1, vec![(0, k("2024"))])
        );
        assert_eq!(
            visual(vec![DrillStep::Next]).drill_levels(3),
            (1, 1, vec![])
        );
        assert_eq!(
            visual(vec![DrillStep::Expand]).drill_levels(3),
            (0, 1, vec![])
        );
        // Expanded, then into "2024 Q1": both levels filtered.
        assert_eq!(
            visual(vec![
                DrillStep::Expand,
                DrillStep::Into {
                    keys: vec![k("2024"), k("Q1")]
                }
            ])
            .drill_levels(3),
            (2, 2, vec![(0, k("2024")), (1, k("Q1"))])
        );
        // Steps past the last level are ignored.
        assert_eq!(
            visual(vec![DrillStep::Next, DrillStep::Next]).drill_levels(2),
            (1, 1, vec![])
        );
    }
}
