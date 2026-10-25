//! The Data view's queries over one model table: pages of rows, counts, distinct values,
//! column statistics and find, all under a view-only sort and filters.
//!
//! Paging never uses OFFSET. An unsorted, unfiltered table is read by rowid range (rowids
//! of a materialised table are dense). Otherwise the view's row order is materialised
//! once as an "index" table of source rowids in an in-memory database; a page is then the
//! index's own rowid range joined back to the source, so any offset costs the same
//! (about 10 ms for 200 rows of a 10-million-row table).

use charted_engine::{Database, sql};
pub use charted_model::FilterOp;
use charted_model::{ColumnKind, DataType, Id, Table};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use ts_rs::TS;

use crate::session::CoreError;
use crate::types;

/// The attached in-memory database holding view indexes.
const VIEW_DB: &str = "charted_view";
/// View indexes kept before the least recently used one is dropped.
const MAX_INDEXES: usize = 8;
pub const MAX_PAGE_ROWS: u32 = 1_000_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SortKey {
    pub column: Id,
    #[serde(default)]
    pub descending: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum Filter {
    /// Keeps rows whose value (as text, the `key` of a distinct value) is listed, or with
    /// `exclude`, rows whose value is not. `null` stands for blank.
    Values {
        values: Vec<Option<String>>,
        #[serde(default)]
        exclude: bool,
    },
    Condition {
        op: FilterOp,
        #[serde(default)]
        value: Option<String>,
        /// The upper bound of `between`.
        #[serde(default)]
        value2: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ColumnFilter {
    pub column: Id,
    pub filter: Filter,
}

/// What the Data view shows: a table, sorted and filtered. View state, never undoable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ViewSpec {
    pub table: Id,
    #[serde(default)]
    pub sort: Vec<SortKey>,
    #[serde(default)]
    pub filters: Vec<ColumnFilter>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ViewCount {
    /// Rows that pass the filters.
    #[ts(type = "number")]
    pub rows: u64,
    /// Distinct values of the requested column among those rows (blank counts as one).
    #[ts(type = "number | null")]
    pub distinct: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DistinctValue {
    /// The value as text: what a `Values` filter lists. `null` is blank.
    pub key: Option<String>,
    /// The value itself, for formatting (numbers stay numbers, dates are ISO text).
    #[ts(type = "unknown")]
    pub value: Value,
    #[ts(type = "number")]
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DistinctValues {
    /// The most frequent values (up to the limit), in ascending value order.
    pub values: Vec<DistinctValue>,
    /// How many distinct values match in total.
    #[ts(type = "number")]
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct HistogramBin {
    /// Bin bounds; for dates and times these are Unix seconds.
    pub start: f64,
    pub end: f64,
    #[ts(type = "number")]
    pub count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TopValue {
    pub value: Option<String>,
    #[ts(type = "number")]
    pub count: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ColumnStats {
    #[ts(type = "number")]
    pub rows: u64,
    /// Non-blank values.
    #[ts(type = "number")]
    pub count: u64,
    #[ts(type = "number")]
    pub distinct: u64,
    #[ts(type = "number")]
    pub nulls: u64,
    /// Empty strings (text columns) or zeros (numeric columns).
    #[ts(type = "number")]
    pub empty: u64,
    #[ts(type = "unknown")]
    pub min: Value,
    #[ts(type = "unknown")]
    pub max: Value,
    pub mean: Option<f64>,
    pub median: Option<f64>,
    pub stddev: Option<f64>,
    pub p25: Option<f64>,
    pub p75: Option<f64>,
    /// Days between the earliest and latest value of a date column.
    #[ts(type = "number | null")]
    pub span_days: Option<i64>,
    #[ts(type = "number | null")]
    pub min_length: Option<u64>,
    #[ts(type = "number | null")]
    pub max_length: Option<u64>,
    pub histogram: Vec<HistogramBin>,
    /// Most frequent values, for text and true/false columns.
    pub top: Vec<TopValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FindResult {
    /// Position of the matching row in the view.
    #[ts(type = "number")]
    pub row: u64,
    pub column: Id,
    /// Rows in the view with at least one match.
    #[ts(type = "number")]
    pub matches: u64,
}

struct Index {
    name: String,
    rows: u64,
    used: u64,
}

/// View indexes and per-table facts, shared by every query of a session.
#[derive(Default)]
pub struct Indexes {
    by_sql: HashMap<String, Index>,
    dense: HashMap<String, (bool, u64)>,
    tick: u64,
    next: u64,
}

pub type SharedIndexes = Arc<Mutex<Indexes>>;

/// Prepares the database for view indexes.
pub fn attach(db: &Database) -> Result<(), CoreError> {
    db.execute(&format!("ATTACH IF NOT EXISTS ':memory:' AS {VIEW_DB}"))?;
    Ok(())
}

struct ViewColumn {
    id: Id,
    name: String,
    data_type: DataType,
    /// The value in the model's type, over source alias `t`.
    expr: String,
    /// What the grid receives: `expr` with types the webview decodes poorly widened.
    display: String,
    /// The physical column, for type-change checks.
    physical: String,
    /// The expression rows are ordered by (a sort-by column's, when set).
    order: String,
}

/// A view resolved against the model and the database schema, ready to query. Building
/// one is cheap; running its queries does not need the session.
pub struct View {
    db: Database,
    indexes: SharedIndexes,
    source: String,
    columns: Vec<ViewColumn>,
    filters: Vec<(Id, String)>,
    order: Vec<String>,
}

/// DuckDB's name for a model data type.
pub fn duckdb_type(t: DataType) -> Option<&'static str> {
    charted_compiler::sqltype(t)
}

fn type_label(t: DataType) -> &'static str {
    match t {
        DataType::String => "text",
        DataType::Int64 => "whole number",
        DataType::Double => "decimal number",
        DataType::Decimal => "fixed decimal number",
        DataType::Boolean => "true/false",
        DataType::Date => "date",
        DataType::Time => "time",
        DataType::DateTime => "date/time",
        DataType::DateTimeTz => "date/time/time zone",
        DataType::Binary => "binary",
        DataType::Unknown => "value",
    }
}

fn as_text(expr: &str) -> String {
    format!("CAST({expr} AS VARCHAR)")
}

impl View {
    pub fn new(
        db: Database,
        indexes: SharedIndexes,
        table: &Table,
        spec: &ViewSpec,
    ) -> Result<Self, CoreError> {
        let source = sql::ident(&table.source);
        let physical: HashMap<String, String> = db
            .describe(&source)?
            .into_iter()
            .map(|c| (c.name, c.duckdb_type))
            .collect();
        let mut columns = Vec::new();
        for c in &table.columns {
            // Calculated columns are stored next to the loaded ones once computed.
            let source_column = match &c.kind {
                ColumnKind::Data { source_column } => source_column.clone(),
                ColumnKind::Calculated { .. } => charted_compiler::calc_column_name(&c.id),
            };
            let source_column = &source_column;
            let Some(ptype) = physical.get(source_column) else {
                continue;
            };
            let raw = format!("t.{}", sql::ident(source_column));
            let expr = match duckdb_type(c.data_type) {
                Some(target) if types::from_duckdb(ptype) != c.data_type => {
                    format!("TRY_CAST({raw} AS {target})")
                }
                _ => raw.clone(),
            };
            let wide = matches!(c.data_type, DataType::Decimal)
                || matches!(
                    ptype.to_ascii_uppercase().as_str(),
                    "HUGEINT" | "UHUGEINT" | "UBIGINT"
                )
                || ptype.to_ascii_uppercase().starts_with("DECIMAL");
            let display = if wide && c.data_type.is_numeric() {
                format!("CAST({expr} AS DOUBLE)")
            } else {
                expr.clone()
            };
            columns.push(ViewColumn {
                id: c.id.clone(),
                name: c.name.clone(),
                data_type: c.data_type,
                order: expr.clone(),
                expr,
                display,
                physical: raw,
            });
        }
        // Sort-by columns order by the other column's expression.
        for c in &table.columns {
            if let Some(by) = &c.sort_by
                && let Some(by_expr) = columns.iter().find(|x| &x.id == by).map(|x| x.expr.clone())
                && let Some(col) = columns.iter_mut().find(|x| x.id == c.id)
            {
                col.order = by_expr;
            }
        }
        let mut view = Self {
            db,
            indexes,
            source,
            columns,
            filters: Vec::new(),
            order: Vec::new(),
        };
        for k in &spec.sort {
            let col = view.column(&k.column)?;
            let collate = if col.data_type == DataType::String && col.order == col.expr {
                " COLLATE nocase"
            } else {
                ""
            };
            // Blank sorts lowest, as in Power BI.
            let dir = if k.descending {
                "DESC NULLS LAST"
            } else {
                "ASC NULLS FIRST"
            };
            view.order.push(format!("{}{collate} {dir}", col.order));
        }
        for f in &spec.filters {
            let cond = view.filter_sql(&f.column, &f.filter)?;
            view.filters.push((f.column.clone(), cond));
        }
        Ok(view)
    }

    fn column(&self, id: &str) -> Result<&ViewColumn, CoreError> {
        self.columns
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| CoreError::Invalid(format!("unknown column {id}")))
    }

    /// A literal of the column's type, rejecting text that does not convert.
    fn typed_literal(&self, col: &ViewColumn, text: &str) -> Result<String, CoreError> {
        let Some(target) = duckdb_type(col.data_type) else {
            return Ok(sql::string(text));
        };
        if col.data_type == DataType::String {
            return Ok(sql::string(text));
        }
        let lit = format!("TRY_CAST({} AS {target})", sql::string(text.trim()));
        let ok = self
            .db
            .query_u64(&format!("SELECT ({lit} IS NOT NULL)::BIGINT"))?;
        if ok == 0 {
            return Err(CoreError::Invalid(format!(
                "\"{text}\" is not a valid {}",
                type_label(col.data_type)
            )));
        }
        Ok(lit)
    }

    fn filter_sql(&self, column: &str, filter: &Filter) -> Result<String, CoreError> {
        let col = self.column(column)?;
        let e = &col.expr;
        Ok(match filter {
            Filter::Values { values, exclude } => {
                let listed: Vec<String> = values.iter().flatten().map(|v| sql::string(v)).collect();
                let blank = values.iter().any(Option::is_none);
                let mut parts = Vec::new();
                if !listed.is_empty() {
                    parts.push(format!("{} IN ({})", as_text(e), listed.join(", ")));
                }
                if blank {
                    parts.push(format!("{e} IS NULL"));
                }
                let any = if parts.is_empty() {
                    "false".to_string()
                } else {
                    format!("({})", parts.join(" OR "))
                };
                if *exclude {
                    format!("NOT coalesce({any}, false)")
                } else {
                    any
                }
            }
            Filter::Condition { op, value, value2 } => {
                let need = |v: &Option<String>| {
                    v.clone()
                        .ok_or_else(|| CoreError::Invalid("the filter needs a value".into()))
                };
                let text = || -> Result<(String, String), CoreError> {
                    Ok((
                        format!("lower({})", as_text(e)),
                        format!("lower({})", sql::string(&need(value)?)),
                    ))
                };
                let is_text = col.data_type == DataType::String;
                match op {
                    FilterOp::IsBlank => format!("({e} IS NULL OR {} = '')", as_text(e)),
                    FilterOp::IsNotBlank => format!("({e} IS NOT NULL AND {} <> '')", as_text(e)),
                    FilterOp::Contains => {
                        let (a, b) = text()?;
                        format!("contains({a}, {b})")
                    }
                    FilterOp::NotContains => {
                        let (a, b) = text()?;
                        format!("NOT coalesce(contains({a}, {b}), false)")
                    }
                    FilterOp::StartsWith => {
                        let (a, b) = text()?;
                        format!("starts_with({a}, {b})")
                    }
                    FilterOp::EndsWith => {
                        let (a, b) = text()?;
                        format!("ends_with({a}, {b})")
                    }
                    FilterOp::Equals | FilterOp::NotEquals if is_text => {
                        let (a, b) = text()?;
                        if *op == FilterOp::Equals {
                            format!("{a} = {b}")
                        } else {
                            format!("{a} IS DISTINCT FROM {b}")
                        }
                    }
                    FilterOp::Between => {
                        let lo = self.typed_literal(col, &need(value)?)?;
                        let hi = self.typed_literal(col, &need(value2)?)?;
                        format!("{e} BETWEEN {lo} AND {hi}")
                    }
                    _ => {
                        let v = self.typed_literal(col, &need(value)?)?;
                        let sym = match op {
                            FilterOp::Equals => "=",
                            FilterOp::NotEquals => "IS DISTINCT FROM",
                            FilterOp::Less => "<",
                            FilterOp::LessOrEqual => "<=",
                            FilterOp::Greater => ">",
                            _ => ">=",
                        };
                        format!("{e} {sym} {v}")
                    }
                }
            }
        })
    }

    /// `WHERE …` for the filters, optionally ignoring one column's own filter.
    fn where_sql(&self, except: Option<&str>) -> String {
        let parts: Vec<&str> = self
            .filters
            .iter()
            .filter(|(c, _)| Some(c.as_str()) != except)
            .map(|(_, s)| s.as_str())
            .collect();
        if parts.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", parts.join(" AND "))
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Indexes> {
        self.indexes.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Whether the source's rowids are exactly 0..rows, and its row count.
    fn dense(&self) -> Result<(bool, u64), CoreError> {
        if let Some(d) = self.lock().dense.get(&self.source) {
            return Ok(*d);
        }
        let rows = self
            .db
            .query_u64(&format!("SELECT count(*) FROM {}", self.source))?;
        let top = self.db.query_u64(&format!(
            "SELECT coalesce(max(rowid) + 1, 0) FROM {}",
            self.source
        ))?;
        let d = (rows == top, rows);
        self.lock().dense.insert(self.source.clone(), d);
        Ok(d)
    }

    /// The view's index table and row count, or `None` when rowids can be used directly.
    fn index(&self) -> Result<Option<(String, u64)>, CoreError> {
        if self.filters.is_empty() && self.order.is_empty() && self.dense()?.0 {
            return Ok(None);
        }
        let mut order = self.order.clone();
        order.push("t.rowid".into());
        let select = format!(
            "SELECT t.rowid AS r FROM {} t{} ORDER BY {}",
            self.source,
            self.where_sql(None),
            order.join(", ")
        );
        // Holding the lock while building stops parallel page requests building twice.
        let mut ix = self.lock();
        ix.tick += 1;
        let tick = ix.tick;
        if let Some(i) = ix.by_sql.get_mut(&select) {
            i.used = tick;
            return Ok(Some((i.name.clone(), i.rows)));
        }
        if ix.by_sql.len() >= MAX_INDEXES
            && let Some(old) = ix
                .by_sql
                .iter()
                .min_by_key(|(_, i)| i.used)
                .map(|(k, _)| k.clone())
            && let Some(i) = ix.by_sql.remove(&old)
        {
            let _ = self.db.execute(&format!("DROP TABLE IF EXISTS {}", i.name));
        }
        ix.next += 1;
        let name = format!("{VIEW_DB}.i{}", ix.next);
        self.db
            .execute(&format!("CREATE OR REPLACE TABLE {name} AS {select}"))?;
        let rows = self.db.query_u64(&format!("SELECT count(*) FROM {name}"))?;
        ix.by_sql.insert(
            select,
            Index {
                name: name.clone(),
                rows,
                used: tick,
            },
        );
        Ok(Some((name, rows)))
    }

    /// `limit` rows from `offset`, as Arrow IPC. Fields are named by column id.
    pub fn rows(
        &self,
        columns: Option<&[Id]>,
        offset: u64,
        limit: u32,
    ) -> Result<Vec<u8>, CoreError> {
        let limit = u64::from(limit.min(MAX_PAGE_ROWS));
        let select = self
            .columns
            .iter()
            .filter(|c| columns.is_none_or(|ids| ids.contains(&c.id)))
            .map(|c| format!("{} AS {}", c.display, sql::ident(&c.id)))
            .collect::<Vec<_>>();
        if select.is_empty() {
            return Err(CoreError::Invalid("no columns to show".into()));
        }
        let select = select.join(", ");
        let (lo, hi) = (offset, offset.saturating_add(limit));
        let query = match self.index()? {
            None => format!(
                "SELECT {select} FROM {} t WHERE t.rowid >= {lo} AND t.rowid < {hi} ORDER BY t.rowid",
                self.source
            ),
            Some((index, _)) => format!(
                "SELECT {select} FROM (SELECT rowid AS pos, r FROM {index} WHERE rowid >= {lo} AND rowid < {hi}) p \
                 JOIN {} t ON t.rowid = p.r ORDER BY p.pos",
                self.source
            ),
        };
        Ok(self.db.arrow(&query)?)
    }

    pub fn count(&self, column: Option<&str>) -> Result<ViewCount, CoreError> {
        let rows = match self.index()? {
            None => self.dense()?.1,
            Some((_, rows)) => rows,
        };
        let distinct = match column {
            Some(id) => {
                let e = &self.column(id)?.expr;
                Some(self.db.query_u64(&format!(
                    "SELECT count(DISTINCT {e}) + coalesce(max(({e} IS NULL)::BIGINT), 0) FROM {} t{}",
                    self.source,
                    self.where_sql(None)
                ))?)
            }
            None => None,
        };
        Ok(ViewCount { rows, distinct })
    }

    /// The most frequent values of a column under the other columns' filters.
    pub fn distinct(
        &self,
        column: &str,
        search: Option<&str>,
        limit: u32,
    ) -> Result<DistinctValues, CoreError> {
        let col = self.column(column)?;
        let e = &col.display;
        let mut conds: Vec<String> = self
            .filters
            .iter()
            .filter(|(c, _)| c != column)
            .map(|(_, s)| s.clone())
            .collect();
        if let Some(s) = search.map(str::trim).filter(|s| !s.is_empty()) {
            conds.push(format!(
                "contains(lower({}), lower({}))",
                as_text(&col.expr),
                sql::string(s)
            ));
        }
        let wh = if conds.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conds.join(" AND "))
        };
        let grouped = format!(
            "SELECT {} AS k, any_value({e}) AS v, count(*) AS n FROM {} t{wh} GROUP BY 1",
            as_text(&col.expr),
            self.source
        );
        let rows = self.db.json_rows(
            &format!(
                "SELECT k, v, n, count(*) OVER () AS total FROM ({grouped}) ORDER BY n DESC, k LIMIT {}",
                limit.clamp(1, 10_000)
            ),
            None,
        )?;
        let mut total = 0;
        let mut values = Vec::new();
        for r in rows {
            let mut o: serde_json::Map<String, Value> = serde_json::from_str(&r)
                .map_err(|e| CoreError::Invalid(format!("unexpected result: {e}")))?;
            total = o.get("total").and_then(Value::as_u64).unwrap_or(0);
            values.push(DistinctValue {
                key: o.get("k").and_then(Value::as_str).map(str::to_string),
                value: o.remove("v").unwrap_or(Value::Null),
                count: o.get("n").and_then(Value::as_u64).unwrap_or(0),
            });
        }
        // Blank first, then ascending by value (numbers numerically).
        values.sort_by(|a, b| match (&a.value, &b.value) {
            (Value::Null, Value::Null) => std::cmp::Ordering::Equal,
            (Value::Null, _) => std::cmp::Ordering::Less,
            (_, Value::Null) => std::cmp::Ordering::Greater,
            (Value::Number(x), Value::Number(y)) => x
                .as_f64()
                .partial_cmp(&y.as_f64())
                .unwrap_or(std::cmp::Ordering::Equal),
            _ => a
                .key
                .as_deref()
                .map(str::to_lowercase)
                .cmp(&b.key.as_deref().map(str::to_lowercase)),
        });
        Ok(DistinctValues { values, total })
    }

    pub fn stats(&self, column: &str) -> Result<ColumnStats, CoreError> {
        let col = self.column(column)?;
        let e = &col.expr;
        let t = col.data_type;
        let wh = self.where_sql(None);
        let src = &self.source;
        let mut fields = vec![
            "count(*) AS rows".to_string(),
            format!("count({e}) AS count"),
            format!("count(DISTINCT {e}) AS distinct"),
        ];
        let numeric = t.is_numeric();
        let temporal = t.is_temporal() || t == DataType::Time;
        if numeric {
            let n = format!("CAST({e} AS DOUBLE)");
            fields.extend([
                format!("count(*) FILTER (WHERE {e} = 0) AS empty"),
                format!("min({n}) AS min"),
                format!("max({n}) AS max"),
                format!("avg({n}) AS mean"),
                format!("median({n}) AS median"),
                format!("stddev_samp({n}) AS stddev"),
                format!("quantile_cont({n}, 0.25) AS p25"),
                format!("quantile_cont({n}, 0.75) AS p75"),
            ]);
        } else if t == DataType::String {
            fields.extend([
                format!("count(*) FILTER (WHERE {e} = '') AS empty"),
                format!("min({e} COLLATE nocase) AS min"),
                format!("max({e} COLLATE nocase) AS max"),
                format!("min(length({e})) AS min_length"),
                format!("max(length({e})) AS max_length"),
            ]);
        } else if temporal || t == DataType::Boolean {
            fields.extend([
                format!("CAST(min({e}) AS VARCHAR) AS min"),
                format!("CAST(max({e}) AS VARCHAR) AS max"),
            ]);
            if t.is_temporal() {
                fields.push(format!(
                    "date_diff('day', CAST(min({e}) AS DATE), CAST(max({e}) AS DATE)) AS span_days"
                ));
            }
        }
        let row = self
            .db
            .json_rows(
                &format!("SELECT {} FROM {src} t{wh}", fields.join(", ")),
                None,
            )?
            .pop()
            .unwrap_or_default();
        let o: serde_json::Map<String, Value> = serde_json::from_str(&row).unwrap_or_default();
        let u = |k: &str| o.get(k).and_then(Value::as_u64);
        let f = |k: &str| o.get(k).and_then(Value::as_f64);
        let rows = u("rows").unwrap_or(0);
        let count = u("count").unwrap_or(0);
        let mut stats = ColumnStats {
            rows,
            count,
            distinct: u("distinct").unwrap_or(0),
            nulls: rows - count.min(rows),
            empty: u("empty").unwrap_or(0),
            min: o.get("min").cloned().unwrap_or(Value::Null),
            max: o.get("max").cloned().unwrap_or(Value::Null),
            mean: f("mean"),
            median: f("median"),
            stddev: f("stddev"),
            p25: f("p25"),
            p75: f("p75"),
            span_days: o.get("span_days").and_then(Value::as_i64),
            min_length: u("min_length"),
            max_length: u("max_length"),
            ..Default::default()
        };

        if numeric || temporal {
            // Equal-width bins; whole numbers over a short range get one bin per value.
            let x = if numeric {
                format!("CAST({e} AS DOUBLE)")
            } else if t == DataType::Time {
                format!("epoch(TIMESTAMP '1970-01-01' + {e})")
            } else {
                format!("epoch(CAST({e} AS TIMESTAMP))")
            };
            let bounds = self.db.json_rows(
                &format!("SELECT min({x}) AS lo, max({x}) AS hi FROM {src} t{wh}"),
                None,
            )?;
            let b: serde_json::Map<String, Value> = bounds
                .first()
                .and_then(|r| serde_json::from_str(r).ok())
                .unwrap_or_default();
            if let (Some(lo), Some(hi)) = (
                b.get("lo").and_then(Value::as_f64),
                b.get("hi").and_then(Value::as_f64),
            ) {
                let (bins, width) = if t == DataType::Int64 && hi - lo < 20.0 {
                    ((hi - lo) as u64 + 1, 1.0)
                } else if hi > lo {
                    (20, (hi - lo) / 20.0)
                } else {
                    (1, 1.0)
                };
                let last = bins - 1;
                let rows = self.db.json_rows(
                    &format!(
                        "SELECT least(floor(({x} - {lo}) / {width})::BIGINT, {last}) AS b, count(*) AS n \
                         FROM {src} t{} GROUP BY 1 ORDER BY 1",
                        if wh.is_empty() {
                            format!(" WHERE {e} IS NOT NULL")
                        } else {
                            format!("{wh} AND {e} IS NOT NULL")
                        }
                    ),
                    None,
                )?;
                let mut counts = vec![0u64; bins as usize];
                for r in rows {
                    let o: serde_json::Map<String, Value> =
                        serde_json::from_str(&r).unwrap_or_default();
                    if let (Some(i), Some(n)) = (
                        o.get("b").and_then(Value::as_u64),
                        o.get("n").and_then(Value::as_u64),
                    ) && let Some(slot) = counts.get_mut(i as usize)
                    {
                        *slot = n;
                    }
                }
                stats.histogram = counts
                    .into_iter()
                    .enumerate()
                    .map(|(i, count)| HistogramBin {
                        start: lo + i as f64 * width,
                        end: lo + (i + 1) as f64 * width,
                        count,
                    })
                    .collect();
            }
        } else {
            let rows = self.db.json_rows(
                &format!(
                    "SELECT {} AS v, count(*) AS n FROM {src} t{wh} GROUP BY 1 ORDER BY 2 DESC, 1 LIMIT 10",
                    as_text(e)
                ),
                None,
            )?;
            stats.top = rows
                .iter()
                .filter_map(|r| serde_json::from_str::<serde_json::Map<String, Value>>(r).ok())
                .map(|o| TopValue {
                    value: o.get("v").and_then(Value::as_str).map(str::to_string),
                    count: o.get("n").and_then(Value::as_u64).unwrap_or(0),
                })
                .collect();
        }
        Ok(stats)
    }

    /// The next row after `after` (or before it, `backwards`) where any of `columns`
    /// contains `text`, compared case-insensitively against the raw values.
    pub fn find(
        &self,
        text: &str,
        columns: Option<&[Id]>,
        after: Option<u64>,
        backwards: bool,
    ) -> Result<Option<FindResult>, CoreError> {
        let needle = sql::string(&text.to_lowercase());
        let cols: Vec<&ViewColumn> = self
            .columns
            .iter()
            .filter(|c| columns.is_none_or(|ids| ids.contains(&c.id)))
            .collect();
        if text.is_empty() || cols.is_empty() {
            return Ok(None);
        }
        let conds: Vec<String> = cols
            .iter()
            .map(|c| format!("contains(lower({}), {needle})", as_text(&c.expr)))
            .collect();
        let any = format!("coalesce({}, false)", conds.join(" OR "));
        let which = format!(
            "CASE {} END",
            conds
                .iter()
                .enumerate()
                .map(|(i, c)| format!("WHEN {c} THEN {i}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let (rel, pos) = match self.index()? {
            None => (format!("{} t", self.source), "t.rowid"),
            Some((index, _)) => (
                format!(
                    "(SELECT rowid AS pos, r FROM {index}) p JOIN {} t ON t.rowid = p.r",
                    self.source
                ),
                "p.pos",
            ),
        };
        let bound = match (after, backwards) {
            (Some(a), false) => format!(" AND {pos} > {a}"),
            (Some(a), true) => format!(" AND {pos} < {a}"),
            (None, _) => String::new(),
        };
        let dir = if backwards { "DESC" } else { "ASC" };
        let hit = self.db.json_rows(
            &format!(
                "SELECT {pos} AS pos, {which} AS col FROM {rel} WHERE {any}{bound} ORDER BY {pos} {dir} LIMIT 1"
            ),
            None,
        )?;
        let Some(o) = hit
            .first()
            .and_then(|r| serde_json::from_str::<serde_json::Map<String, Value>>(r).ok())
        else {
            return Ok(None);
        };
        let matches = self
            .db
            .query_u64(&format!("SELECT count(*) FROM {rel} WHERE {any}"))?;
        let col = o.get("col").and_then(Value::as_u64).unwrap_or(0) as usize;
        Ok(Some(FindResult {
            row: o.get("pos").and_then(Value::as_u64).unwrap_or(0),
            column: cols.get(col).map(|c| c.id.clone()).unwrap_or_default(),
            matches,
        }))
    }

    /// Rejects a type change when some values cannot be converted.
    pub fn check_type_change(&self, column: &str, to: DataType) -> Result<(), CoreError> {
        let col = self.column(column)?;
        let Some(target) = duckdb_type(to) else {
            return Err(CoreError::Invalid(
                "charted cannot convert to that type".into(),
            ));
        };
        let p = &col.physical;
        let bad = self.db.json_rows(
            &format!(
                "SELECT count(*) AS n, any_value(CAST({p} AS VARCHAR)) AS example FROM {} t \
                 WHERE {p} IS NOT NULL AND TRY_CAST({p} AS {target}) IS NULL",
                self.source
            ),
            None,
        )?;
        let o: serde_json::Map<String, Value> = bad
            .first()
            .and_then(|r| serde_json::from_str(r).ok())
            .unwrap_or_default();
        let n = o.get("n").and_then(Value::as_u64).unwrap_or(0);
        if n > 0 {
            let example = o.get("example").and_then(Value::as_str).unwrap_or("");
            return Err(CoreError::Invalid(format!(
                "{} can't become a {}: {n} value(s) don't convert, such as \"{example}\"",
                col.name,
                type_label(to)
            )));
        }
        Ok(())
    }

    /// Rejects sorting `column` by `by` unless each value has exactly one sort value.
    pub fn check_sort_by(&self, column: &str, by: &str) -> Result<(), CoreError> {
        let col = self.column(column)?;
        let other = self.column(by)?;
        let bad = self.db.json_rows(
            &format!(
                "SELECT CAST({a} AS VARCHAR) AS v, count(DISTINCT {b}) AS n FROM {} t \
                 GROUP BY {a} HAVING count(DISTINCT {b}) > 1 LIMIT 1",
                self.source,
                a = col.expr,
                b = other.expr
            ),
            None,
        )?;
        if let Some(o) = bad
            .first()
            .and_then(|r| serde_json::from_str::<serde_json::Map<String, Value>>(r).ok())
        {
            let v = o.get("v").and_then(Value::as_str).unwrap_or("(blank)");
            let n = o.get("n").and_then(Value::as_u64).unwrap_or(0);
            return Err(CoreError::Invalid(format!(
                "{} can't be sorted by {}: \"{v}\" has {n} different {} values. Each value needs exactly one.",
                col.name, other.name, other.name
            )));
        }
        Ok(())
    }
}
