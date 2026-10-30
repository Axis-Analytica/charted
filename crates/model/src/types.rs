use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A stable identifier (Power BI calls these lineage tags). Names change; ids never do.
pub type Id = String;

/// Interchange data charted does not interpret but must round-trip losslessly, such as
/// another tool's expression dialects, foreign vendor extensions or Power BI properties
/// charted has no UI for yet. Keys are namespaced (`ossie.*`, `powerbi.*`).
pub type Extras = std::collections::BTreeMap<String, serde_json::Value>;

/// The language an expression is written in. Expressions authored in charted are DAX;
/// models imported from other tools may carry SQL instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[ts(export)]
pub enum ExpressionLanguage {
    #[default]
    Dax,
    Sql,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Model {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub tables: Vec<Table>,
    #[serde(default)]
    pub relationships: Vec<Relationship>,
    /// Row-level security roles (what *View as* previews).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<Role>,
    #[serde(default, skip_serializing_if = "Extras::is_empty")]
    #[ts(type = "Record<string, unknown>")]
    pub extras: Extras,
    /// The report built on this model. It is stored in its own files, not in the Ossie
    /// document, but lives here so model and report edits share one undo history.
    #[serde(default)]
    pub report: crate::report::Report,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Table {
    pub id: Id,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Where the rows come from: the DuckDB table that a load materialised.
    pub source: String,
    #[serde(default)]
    pub columns: Vec<Column>,
    #[serde(default)]
    pub measures: Vec<Measure>,
    /// Drill paths through the table's columns (Year → Quarter → Month).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hierarchies: Vec<Hierarchy>,
    #[serde(default)]
    pub hidden: bool,
    /// Marked as the model's date table (required by time intelligence).
    #[serde(default)]
    pub is_date_table: bool,
    /// Set on a calculated table: its rows come from a DAX table expression, which
    /// charted materialises into `source` (see [`calculated_table_source`]). Its data
    /// columns are the expression's result columns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calculated: Option<CalculatedTable>,
    #[serde(default, skip_serializing_if = "Extras::is_empty")]
    #[ts(type = "Record<string, unknown>")]
    pub extras: Extras,
}

/// A calculated table's definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CalculatedTable {
    pub expression: String,
    #[serde(default)]
    pub language: ExpressionLanguage,
}

/// The DuckDB table a calculated table's rows are materialised into.
pub fn calculated_table_source(table_id: &str) -> String {
    format!("__calc_table_{table_id}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum DataType {
    String,
    Int64,
    Double,
    Decimal,
    Boolean,
    Date,
    Time,
    DateTime,
    DateTimeTz,
    Binary,
    Unknown,
}

impl DataType {
    pub fn is_numeric(self) -> bool {
        matches!(self, DataType::Int64 | DataType::Double | DataType::Decimal)
    }

    pub fn is_temporal(self) -> bool {
        matches!(
            self,
            DataType::Date | DataType::DateTime | DataType::DateTimeTz
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[ts(export)]
pub enum Summarize {
    #[default]
    Default,
    None,
    Sum,
    Average,
    Min,
    Max,
    Count,
    DistinctCount,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum ColumnKind {
    /// A physical column of the source table.
    Data { source_column: String },
    /// A DAX expression evaluated per row.
    Calculated {
        expression: String,
        #[serde(default)]
        language: ExpressionLanguage,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Column {
    pub id: Id,
    pub name: String,
    pub data_type: DataType,
    pub kind: ColumnKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format_string: Option<String>,
    #[serde(default)]
    pub summarize_by: Summarize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_category: Option<String>,
    /// Id of the column whose values order this one (e.g. MonthName by MonthNumber).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_by: Option<Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_folder: Option<String>,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub is_key: bool,
    #[serde(default, skip_serializing_if = "Extras::is_empty")]
    #[ts(type = "Record<string, unknown>")]
    pub extras: Extras,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Measure {
    pub id: Id,
    pub name: String,
    /// The authored expression, normally DAX.
    pub expression: String,
    #[serde(default)]
    pub language: ExpressionLanguage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format_string: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_folder: Option<String>,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "Extras::is_empty")]
    #[ts(type = "Record<string, unknown>")]
    pub extras: Extras,
}

/// A named, ordered list of a table's columns that visuals drill through.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Hierarchy {
    pub id: Id,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// From the top (e.g. Year) down.
    pub levels: Vec<Level>,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_folder: Option<String>,
    #[serde(default, skip_serializing_if = "Extras::is_empty")]
    #[ts(type = "Record<string, unknown>")]
    pub extras: Extras,
}

/// One level of a hierarchy: a column of the hierarchy's table, under a name of its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Level {
    pub name: String,
    /// The column's id.
    pub column: Id,
}

/// A row-level security role: who it's for and which rows of each table they see.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Role {
    pub id: Id,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// At most one per table. Tables without a filter are not restricted by the role.
    #[serde(default)]
    pub filters: Vec<RoleFilter>,
    /// Members (users or groups) as Power BI names them; kept for the round trip.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<String>,
    #[serde(default, skip_serializing_if = "Extras::is_empty")]
    #[ts(type = "Record<string, unknown>")]
    pub extras: Extras,
}

/// A role's filter on a table: a DAX condition evaluated per row (`[Region] = "North"`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RoleFilter {
    pub table: Id,
    pub expression: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum Cardinality {
    One,
    Many,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[ts(export)]
pub enum CrossFilter {
    /// Filters flow from the "one" side to the "many" side.
    #[default]
    Single,
    Both,
}

/// A relationship between two columns. `from` is the many side (the fact table).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Relationship {
    pub id: Id,
    pub from_table: Id,
    pub from_column: Id,
    pub to_table: Id,
    pub to_column: Id,
    pub from_cardinality: Cardinality,
    pub to_cardinality: Cardinality,
    #[serde(default)]
    pub cross_filter: CrossFilter,
    pub active: bool,
    #[serde(default, skip_serializing_if = "Extras::is_empty")]
    #[ts(type = "Record<string, unknown>")]
    pub extras: Extras,
}

impl Model {
    pub fn table(&self, id: &str) -> Option<&Table> {
        self.tables.iter().find(|t| t.id == id)
    }

    pub fn table_mut(&mut self, id: &str) -> Option<&mut Table> {
        self.tables.iter_mut().find(|t| t.id == id)
    }

    pub fn table_by_name(&self, name: &str) -> Option<&Table> {
        self.tables
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(name))
    }

    pub fn role(&self, id: &str) -> Option<&Role> {
        self.roles.iter().find(|r| r.id == id)
    }

    pub fn relationship(&self, id: &str) -> Option<&Relationship> {
        self.relationships.iter().find(|r| r.id == id)
    }
}

impl Table {
    pub fn column(&self, id: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.id == id)
    }

    pub fn column_mut(&mut self, id: &str) -> Option<&mut Column> {
        self.columns.iter_mut().find(|c| c.id == id)
    }

    pub fn column_by_name(&self, name: &str) -> Option<&Column> {
        self.columns
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn measure_mut(&mut self, id: &str) -> Option<&mut Measure> {
        self.measures.iter_mut().find(|m| m.id == id)
    }

    pub fn hierarchy(&self, id: &str) -> Option<&Hierarchy> {
        self.hierarchies.iter().find(|h| h.id == id)
    }

    /// True when `name` is already used by a column, measure or hierarchy of this table.
    pub fn has_field_named(&self, name: &str, except: Option<&str>) -> bool {
        let clash = |id: &str, n: &str| Some(id) != except && n.eq_ignore_ascii_case(name);
        self.columns.iter().any(|c| clash(&c.id, &c.name))
            || self.measures.iter().any(|m| clash(&m.id, &m.name))
            || self.hierarchies.iter().any(|h| clash(&h.id, &h.name))
    }
}
