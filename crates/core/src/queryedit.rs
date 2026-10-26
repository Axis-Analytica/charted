//! The query editor's view of a query at one of its steps: a preview of the rows, the
//! columns with their types, the SQL and M it compiles to, column profiles, and the
//! lookups a step needs when it is added (the header row, how many parts a split makes,
//! a column's values).

use charted_compiler::steps::{self, Query, Step};
use charted_engine::{Database, sql};
use charted_model::DataType;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::session::CoreError;
use crate::types::from_duckdb;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QueryColumn {
    pub name: String,
    pub data_type: DataType,
    pub duckdb_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QueryInfo {
    pub columns: Vec<QueryColumn>,
    /// The SQL up to the step, as DuckDB runs it.
    pub sql: String,
    /// The whole query as M (Power Query's language).
    pub m: String,
    /// Display names of the source and each step.
    pub step_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ValueCount {
    /// The value as text (`null` for blank).
    pub value: Option<String>,
    #[ts(type = "number")]
    pub count: u64,
}

/// Power Query's column quality and distribution, over the profiled rows.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ColumnProfile {
    pub name: String,
    #[ts(type = "number")]
    pub count: u64,
    /// Blank (null or empty text).
    #[ts(type = "number")]
    pub empty: u64,
    /// Values a type change couldn't convert (only for a type change step).
    #[ts(type = "number")]
    pub errors: u64,
    #[ts(type = "number")]
    pub distinct: u64,
    /// Values that occur exactly once.
    #[ts(type = "number")]
    pub unique: u64,
    pub min: Option<String>,
    pub max: Option<String>,
    /// The most frequent values, most frequent first.
    pub top: Vec<ValueCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QueryProfile {
    /// Rows profiled (the first 1,000, or all).
    #[ts(type = "number")]
    pub rows: u64,
    pub columns: Vec<ColumnProfile>,
}

/// A lookup made while adding a step.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum Analysis {
    /// The first row's values, as text (for Use First Row as Headers).
    FirstRow,
    /// The most parts any value splits into.
    SplitParts { column: String, delimiter: String },
    /// A column's distinct values, as text, in order.
    Distinct { column: String, limit: u32 },
    /// The narrowest type each text column's values all convert to (Power Query's
    /// automatic "Changed Type" after a text file's headers), over the first 1,000 rows.
    DetectTypes,
}

/// The SQL of a query up to `upto` steps, in a draft set of queries.
pub struct Prepared {
    pub db: Database,
    pub sql: String,
    /// The step before, when the step at `upto` changes types (its errors are counted).
    pub type_change: Option<(String, Vec<String>)>,
    /// The query's numbers are written "1.234,5".
    pub decimal_comma: bool,
}

pub fn info(p: &Prepared, query: &Query, queries: &[Query]) -> Result<QueryInfo, CoreError> {
    let cols = p.db.describe(&format!("({})", p.sql))?;
    Ok(QueryInfo {
        columns: cols
            .into_iter()
            .map(|c| QueryColumn {
                data_type: from_duckdb(&c.duckdb_type),
                name: c.name,
                duckdb_type: c.duckdb_type,
            })
            .collect(),
        sql: p.sql.clone(),
        m: steps::to_m(query, queries),
        step_names: query.step_display_names(),
    })
}

/// The first `limit` rows as Arrow, with types the UI decodes well (decimals and huge
/// integers as doubles).
pub fn preview(p: &Prepared, limit: u32) -> Result<Vec<u8>, CoreError> {
    let cols = p.db.describe(&format!("({})", p.sql))?;
    let select: Vec<String> = cols
        .iter()
        .map(|c| {
            let t = c.duckdb_type.to_ascii_uppercase();
            let e = sql::ident(&c.name);
            if t.starts_with("DECIMAL") || t == "HUGEINT" || t == "UHUGEINT" {
                format!("CAST({e} AS DOUBLE) AS {e}")
            } else if t.starts_with("STRUCT")
                || t.starts_with("MAP")
                || t.ends_with("[]")
                || t == "BLOB"
            {
                format!("CAST({e} AS VARCHAR) AS {e}")
            } else {
                e
            }
        })
        .collect();
    if select.is_empty() {
        return Ok(p.db.arrow("SELECT 1 WHERE FALSE")?);
    }
    Ok(p.db.arrow(&format!(
        "SELECT {} FROM ({}) AS q LIMIT {}",
        select.join(", "),
        p.sql,
        limit.clamp(1, 10_000)
    ))?)
}

fn rows(db: &Database, sql_text: &str) -> Result<Vec<serde_json::Map<String, Value>>, CoreError> {
    Ok(db
        .json_rows(sql_text, None)?
        .into_iter()
        .filter_map(|r| serde_json::from_str(&r).ok())
        .collect())
}

fn text(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

fn count(v: Option<&Value>) -> u64 {
    v.and_then(Value::as_u64).unwrap_or(0)
}

/// Profiles the first 1,000 rows (or every row) of each column.
pub fn profile(p: &Prepared, full: bool) -> Result<QueryProfile, CoreError> {
    let cols = p.db.describe(&format!("({})", p.sql))?;
    let limit = if full {
        String::new()
    } else {
        " LIMIT 1000".into()
    };
    let data = format!("SELECT * FROM ({}) AS q{limit}", p.sql);
    let n = p.db.query_u64(&format!("SELECT count(*) FROM ({data})"))?;
    let mut out = Vec::new();
    for c in cols.iter().take(80) {
        let e = sql::ident(&c.name);
        let stats = rows(
            &p.db,
            &format!(
                "SELECT count(*) FILTER (WHERE {e} IS NULL OR trim(CAST({e} AS VARCHAR)) = '') AS empty, count(DISTINCT {e}) AS distinct, CAST(min({e}) AS VARCHAR) AS min, CAST(max({e}) AS VARCHAR) AS max FROM ({data}) AS d"
            ),
        )?;
        let s = stats.first().cloned().unwrap_or_default();
        let unique = p.db.query_u64(&format!(
            "SELECT count(*) FROM (SELECT {e} FROM ({data}) AS d GROUP BY {e} HAVING count(*) = 1)"
        ))?;
        let top = rows(
            &p.db,
            &format!(
                "SELECT CAST({e} AS VARCHAR) AS v, count(*) AS n FROM ({data}) AS d GROUP BY 1 ORDER BY n DESC, v NULLS LAST LIMIT 12"
            ),
        )?
        .into_iter()
        .map(|r| ValueCount {
            value: text(r.get("v")),
            count: count(r.get("n")),
        })
        .collect();
        out.push(ColumnProfile {
            name: c.name.clone(),
            count: n,
            empty: count(s.get("empty")),
            errors: 0,
            distinct: count(s.get("distinct")),
            unique,
            min: text(s.get("min")),
            max: text(s.get("max")),
            top,
        });
    }
    // A type change's errors: values present before the step but blank after it. The
    // step keeps the rows in order, so they pair up by position.
    if let Some((before, changed)) = &p.type_change {
        for c in out.iter_mut().filter(|c| changed.contains(&c.name)) {
            let e = sql::ident(&c.name);
            c.errors = p.db.query_u64(&format!(
                "WITH a AS (SELECT {e} AS x, row_number() OVER () AS r FROM ({before}) AS q{limit}), b AS (SELECT {e} AS x, row_number() OVER () AS r FROM ({data}) AS q) SELECT count(*) FROM a JOIN b USING (r) WHERE a.x IS NOT NULL AND trim(CAST(a.x AS VARCHAR)) <> '' AND b.x IS NULL"
            ))?;
        }
    }
    Ok(QueryProfile {
        rows: n,
        columns: out,
    })
}

pub fn analyze(p: &Prepared, a: &Analysis) -> Result<Value, CoreError> {
    match a {
        Analysis::FirstRow => {
            let cols = p.db.describe(&format!("({})", p.sql))?;
            if cols.is_empty() {
                return Ok(Value::Array(vec![]));
            }
            let list: Vec<String> = cols
                .iter()
                .map(|c| format!("CAST({} AS VARCHAR)", sql::ident(&c.name)))
                .collect();
            let r = rows(
                &p.db,
                &format!(
                    "SELECT [{}] AS v FROM ({}) AS q LIMIT 1",
                    list.join(", "),
                    p.sql
                ),
            )?;
            Ok(r.first()
                .and_then(|r| r.get("v").cloned())
                .unwrap_or(Value::Array(vec![])))
        }
        Analysis::SplitParts { column, delimiter } => {
            let n = p.db.query_u64(&format!(
                "SELECT coalesce(max(len(string_split(CAST({} AS VARCHAR), {}))), 1) FROM ({}) AS q",
                sql::ident(column),
                sql::string(delimiter),
                p.sql
            ))?;
            Ok(Value::from(n))
        }
        Analysis::DetectTypes => {
            let cols = p.db.describe(&format!("({})", p.sql))?;
            let sample = format!("(SELECT * FROM ({}) AS q LIMIT 1000)", p.sql);
            let mut out = Vec::new();
            for c in cols {
                let e = sql::ident(&c.name);
                let mut ty = from_duckdb(&c.duckdb_type);
                if ty == DataType::String {
                    // Narrowest first; a type fits when every non-blank value converts.
                    for (cand, sql_ty) in [
                        (DataType::Boolean, "BOOLEAN"),
                        (DataType::Int64, "BIGINT"),
                        (DataType::Double, "DOUBLE"),
                        (DataType::Date, "DATE"),
                        (DataType::DateTime, "TIMESTAMP"),
                    ] {
                        // Numbers written "1.234,5" read that way (as Change type will).
                        let v = if p.decimal_comma
                            && matches!(cand, DataType::Int64 | DataType::Double)
                        {
                            charted_compiler::steps::comma_number_text(&e)
                        } else {
                            format!("trim({e})")
                        };
                        // DuckDB rounds "2.5" to a whole number; that's a decimal.
                        let fits = if cand == DataType::Int64 {
                            format!(
                                "TRY_CAST({v} AS BIGINT) IS NOT NULL AND TRY_CAST({v} AS DOUBLE) = TRY_CAST({v} AS BIGINT)"
                            )
                        } else {
                            format!("TRY_CAST({v} AS {sql_ty}) IS NOT NULL")
                        };
                        let bad = p.db.query_u64(&format!(
                            "SELECT count(*) FILTER (WHERE trim({e}) <> '' AND NOT ({fits})) + (count(*) FILTER (WHERE trim({e}) <> '') = 0)::INT FROM {sample}"
                        ))?;
                        if bad == 0 {
                            ty = cand;
                            break;
                        }
                    }
                }
                out.push(serde_json::json!([c.name, ty]));
            }
            Ok(Value::Array(out))
        }
        Analysis::Distinct { column, limit } => {
            let e = sql::ident(column);
            let r = rows(
                &p.db,
                &format!(
                    "SELECT DISTINCT CAST({e} AS VARCHAR) AS v FROM ({}) AS q WHERE {e} IS NOT NULL ORDER BY 1 LIMIT {}",
                    p.sql,
                    (*limit).clamp(1, 1000)
                ),
            )?;
            Ok(Value::Array(
                r.into_iter().filter_map(|r| r.get("v").cloned()).collect(),
            ))
        }
    }
}

/// When the step at `upto` changes types: the SQL before it and the columns it changes.
pub fn type_change_before(
    query: &Query,
    upto: usize,
    sql_before: impl FnOnce() -> Result<String, CoreError>,
) -> Result<Option<(String, Vec<String>)>, CoreError> {
    match upto.checked_sub(1).and_then(|i| query.steps.get(i)) {
        Some(Step::ChangeTypes { columns }) => Ok(Some((
            sql_before()?,
            columns.iter().map(|(c, _)| c.clone()).collect(),
        ))),
        _ => Ok(None),
    }
}
