//! Checks a prospective relationship against the data: how many keys each side has,
//! whether they are unique, how many from-side keys find no match, and the cardinality
//! Power BI would suggest.

use charted_engine::{Database, sql};
use charted_model::{Cardinality, Model};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::session::CoreError;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RelationshipCheck {
    #[ts(type = "number")]
    pub from_rows: u64,
    #[ts(type = "number")]
    pub from_distinct: u64,
    #[ts(type = "number")]
    pub to_rows: u64,
    #[ts(type = "number")]
    pub to_distinct: u64,
    /// Distinct from-side keys with no row on the to side.
    #[ts(type = "number")]
    pub unmatched: u64,
    pub from_cardinality: Cardinality,
    pub to_cardinality: Cardinality,
}

fn column(model: &Model, table: &str, column: &str) -> Result<String, CoreError> {
    let t = model
        .table(table)
        .ok_or_else(|| CoreError::Invalid(format!("no table {table}")))?;
    let c = t
        .column(column)
        .ok_or_else(|| CoreError::Invalid(format!("no column {column} in {}", t.name)))?;
    let physical = match &c.kind {
        charted_model::ColumnKind::Data { source_column } => source_column.clone(),
        charted_model::ColumnKind::Calculated { .. } => charted_compiler::calc_column_name(&c.id),
    };
    Ok(format!(
        "SELECT {} AS k FROM {} WHERE {0} IS NOT NULL",
        sql::ident(&physical),
        sql::ident(&t.source)
    ))
}

/// The SQL that measures both sides (one row of six counts).
pub fn plan(model: &Model, ft: &str, fc: &str, tt: &str, tc: &str) -> Result<String, CoreError> {
    let f = column(model, ft, fc)?;
    let t = column(model, tt, tc)?;
    Ok(format!(
        "WITH f AS ({f}), t AS ({t}) SELECT \
         (SELECT count(*) FROM f) AS fr, (SELECT count(DISTINCT k) FROM f) AS fd, \
         (SELECT count(*) FROM t) AS tr, (SELECT count(DISTINCT k) FROM t) AS td, \
         (SELECT count(DISTINCT k) FROM f WHERE k NOT IN (SELECT k FROM t)) AS um"
    ))
}

pub fn run(db: &Database, sql_text: &str) -> Result<RelationshipCheck, CoreError> {
    let rows = db.json_rows(sql_text, Some(1))?;
    let o: serde_json::Value = rows
        .first()
        .and_then(|r| serde_json::from_str(r).ok())
        .unwrap_or_default();
    let n = |k: &str| o.get(k).and_then(serde_json::Value::as_u64).unwrap_or(0);
    let (fr, fd, tr, td) = (n("fr"), n("fd"), n("tr"), n("td"));
    let unique = |rows: u64, distinct: u64| rows == distinct;
    // Prefer many-to-one with the unique side as the "to" side, as Power BI does.
    let (from_cardinality, to_cardinality) = match (unique(fr, fd), unique(tr, td)) {
        (true, true) => (Cardinality::One, Cardinality::One),
        (false, true) => (Cardinality::Many, Cardinality::One),
        (true, false) => (Cardinality::One, Cardinality::Many),
        (false, false) => (Cardinality::Many, Cardinality::Many),
    };
    Ok(RelationshipCheck {
        from_rows: fr,
        from_distinct: fd,
        to_rows: tr,
        to_distinct: td,
        unmatched: n("um"),
        from_cardinality,
        to_cardinality,
    })
}
