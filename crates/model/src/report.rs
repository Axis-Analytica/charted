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
}

impl Visual {
    /// Every field the visual uses, in role order.
    pub fn projections(&self) -> impl Iterator<Item = (&str, &Projection)> {
        self.roles
            .iter()
            .flat_map(|(r, ps)| ps.iter().map(move |p| (r.as_str(), p)))
    }
}
