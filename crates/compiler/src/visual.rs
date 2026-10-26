//! Compiles a visual's query (grouping columns, values, filters, order) to one DuckDB
//! SELECT with DAX semantics. `interp.rs` is the reference these must agree with.
//!
//! How it works:
//! - The visual's grouping columns are the keys. Every value is lowered to SQL over a
//!   *frame*: a relation with one row per key combination (or, inside an iterator
//!   that needs context transition, per key combination and iterated row).
//! - Every aggregate is computed in a filter context: the keys it is grouped by, the
//!   filters on it and the relationships in force. Each distinct (table, context,
//!   aggregate) becomes a CTE over that table, left-joined many-to-one to the tables it
//!   reaches, grouped by the keys it can reach and filtered by the filters it can
//!   reach. Keys and filters it cannot reach do not affect it (single-direction
//!   propagation), so its value repeats across them.
//! - CALCULATE changes the context: ALL removes keys and filters, filter arguments
//!   replace them, a row context becomes keys (context transition). A measure is
//!   CALCULATE around its expression.
//! - Iterators over plain rows (`SUMX ( Sales, Sales[Qty] * RELATED ( … ) )`) are
//!   aggregates of a row expression. Iterators whose expression needs context
//!   transition get a frame of their own: the outer frame joined to the iterated rows,
//!   with the expression evaluated per row and aggregated back.
//! - The result rows are the key combinations where some aggregate has rows (each
//!   CTE's keys, crossed with the distinct values of keys it cannot reach), minus rows
//!   where every value is blank, like SUMMARIZECOLUMNS.
//! - BLANK follows DAX: `+`/`-` treat it as zero unless both sides are blank, DIVIDE by
//!   zero or blank gives the alternate result, counts of nothing are blank.

use charted_dax::{
    AggFunc, Anchor, BinaryOp, Bound, DateOp, DatePart, FilterArg, FormatSpec, InfoFunc, Period,
    ScalarFunc, TableExpr, UnaryOp,
};
use charted_model::{
    Aggregation, ColumnKind, Condition, DataType, DateUnit, FieldRef, FilterKind, FilterOp, Id,
    Model, Projection, RelativePeriod, Table,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use ts_rs::TS;

use crate::graph::Reach;
use crate::quote::{ident, string};

/// The physical storage of the loaded tables, which the model may cast.
pub trait Catalog {
    /// The DuckDB type of a loaded column (as `DataType`), if the column exists.
    fn physical_type(&self, source: &str, column: &str) -> Option<DataType>;
}

impl<F: Fn(&str, &str) -> Option<DataType>> Catalog for F {
    fn physical_type(&self, source: &str, column: &str) -> Option<DataType> {
        self(source, column)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QueryFilter {
    pub field: Projection,
    pub filter: FilterKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", content = "index", rename_all = "camelCase")]
#[ts(export)]
pub enum OrderTarget {
    Group(usize),
    Value(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct OrderBy {
    pub target: OrderTarget,
    #[serde(default)]
    pub descending: bool,
}

/// What a visual asks for. Output columns are named `g0…` for groups and `v0…` for
/// values, in this order.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VisualQuery {
    /// Columns to group by (projections whose field is a column).
    #[serde(default)]
    pub groups: Vec<Projection>,
    /// Measures, or columns with an aggregation.
    #[serde(default)]
    pub values: Vec<Projection>,
    #[serde(default)]
    pub filters: Vec<QueryFilter>,
    /// When empty, groups ascend in their sort order.
    #[serde(default)]
    pub order: Vec<OrderBy>,
    #[serde(default)]
    pub limit: Option<u32>,
    /// Also return each group's value as text (`k0…`, after the values), as filters
    /// compare it: selections in a visual become filters on these.
    #[serde(default)]
    pub keys: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum CompileError {
    #[error("{0}")]
    Field(String),
    #[error("the measure [{measure}] has an error: {message}")]
    Measure { measure: String, message: String },
    #[error("[{0}] refers to itself through other measures")]
    Circular(String),
    #[error(
        "there is more than one active path between '{from}' and '{to}'; deactivate one of the relationships"
    )]
    Ambiguous { from: String, to: String },
    #[error("{0} isn't supported yet")]
    Unsupported(String),
}

type Result<T> = std::result::Result<T, CompileError>;

/// The value kinds that matter for SQL generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum ValueType {
    Number,
    Text,
    Bool,
    Date,
}

impl ValueType {
    fn of(t: DataType) -> Self {
        match t {
            DataType::String | DataType::Binary | DataType::Unknown => ValueType::Text,
            DataType::Boolean => ValueType::Bool,
            DataType::Date | DataType::DateTime | DataType::DateTimeTz | DataType::Time => {
                ValueType::Date
            }
            _ => ValueType::Number,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Compiled {
    pub sql: String,
    /// The type of each value column (`v0…`).
    pub value_types: Vec<ValueType>,
}

/// What an aggregate over a table sees: its grouping keys (with their SQL), its
/// filter predicates, its FROM clause, and whether the joins can repeat its rows
/// (a both-directions relationship crossed from the one side).
type Scope = (Vec<(Key, String)>, Vec<String>, String, bool);

/// A frame column holding a column's value for the current frame row.
#[derive(Debug, Clone, PartialEq)]
struct Key {
    name: String,
    table: Id,
    column: Id,
}

/// How a filter restricts rows of its table.
#[derive(Debug, Clone, PartialEq)]
enum Pred {
    /// A row condition over the table's columns.
    Cond(Bound),
    /// A visual or filter-pane filter on one column.
    Kind(Id, FilterKind),
    /// SQL over one column, `\u{7}` standing for the column.
    Sql(Id, String),
    /// The columns' values are among the rows of a value set CTE, matched on the keys
    /// the set was computed for (VALUES as a CALCULATE filter).
    InSet {
        set: usize,
        columns: Vec<Id>,
        keys: Vec<Key>,
    },
}

#[derive(Debug, Clone, PartialEq)]
struct CFilter {
    table: Id,
    /// The columns it filters (replaced by a later filter on any of them).
    columns: Vec<Id>,
    pred: Pred,
}

#[derive(Debug, Clone, Default)]
struct Ctx {
    keys: Vec<Key>,
    filters: Vec<CFilter>,
    use_rels: Vec<Id>,
}

impl Ctx {
    fn touches(&self, columns: &[Id], expanded: &[Id]) -> (Vec<usize>, Vec<usize>) {
        let k = (0..self.keys.len())
            .filter(|&i| {
                if columns.is_empty() {
                    expanded.contains(&self.keys[i].table)
                } else {
                    columns.contains(&self.keys[i].column)
                }
            })
            .collect();
        let f = (0..self.filters.len())
            .filter(|&i| {
                if columns.is_empty() {
                    expanded.contains(&self.filters[i].table)
                } else {
                    self.filters[i].columns.iter().any(|c| columns.contains(c))
                }
            })
            .collect();
        (k, f)
    }

    fn remove(&mut self, keys: &[usize], filters: &[usize]) {
        let mut i = 0;
        self.keys.retain(|_| {
            i += 1;
            !keys.contains(&(i - 1))
        });
        let mut j = 0;
        self.filters.retain(|_| {
            j += 1;
            !filters.contains(&(j - 1))
        });
    }
}

/// Values of an iterated row available in a frame: model column → frame column.
#[derive(Debug, Clone)]
struct RowBinding {
    table: Id,
    /// The row's columns that can be read: model column → frame column.
    columns: Vec<(Id, String)>,
    /// The columns that identify the row (its key, or all of them): context transition
    /// filters on these.
    identity: Vec<Id>,
    /// Every column the row has: context transition replaces the filters on them.
    covers: Vec<Id>,
    /// RELATED lookups: (table, column) → frame column.
    related: Vec<((Id, Id), String)>,
}

/// A relation the scalars of one level are evaluated over.
#[derive(Debug, Clone)]
struct Frame {
    /// A CTE name.
    rel: String,
    keys: Vec<Key>,
}

/// An iterated table's rows joined to a frame (`Compiler::row_frame`).
struct RowFrame {
    frame: Frame,
    /// The outer rows' bindings and the iterated row's, last.
    rows: Vec<RowBinding>,
    /// FILTER conditions evaluated over the frame (unrendered).
    conds: Vec<String>,
}

/// SQL for a value, with `\u{1}n\u{1}` standing for CTE n's value column, and `f.` for
/// the frame.
#[derive(Debug, Clone)]
struct Scalar {
    sql: String,
    ty: ValueType,
}

struct CteDef {
    name: String,
    sql: String,
    /// Frame key names it is grouped by (joined on).
    keys: Vec<String>,
    /// Computed under the visual's own filters, so its key combinations are ones the
    /// visual can show (otherwise they are checked against the key domains).
    exact: bool,
}

struct Compiler<'a> {
    model: &'a Model,
    catalog: &'a dyn Catalog,
    ctes: Vec<CteDef>,
    /// Variables: name → value in the frame they were defined in.
    vars: Vec<(String, Scalar)>,
    /// Measures being expanded (cycle detection).
    stack: Vec<Id>,
    /// The visual's keys and its filters, for the key-domain CTE.
    top_keys: Vec<Key>,
    top_filters: Vec<CFilter>,
    dom: Option<String>,
    dks: Vec<(Id, String, Vec<String>)>,
    counter: usize,
}

fn alias(model: &Model, table: &str) -> String {
    let i = model.tables.iter().position(|t| t.id == table).unwrap_or(0);
    format!("t{i}")
}

fn table<'a>(model: &'a Model, id: &str) -> Result<&'a Table> {
    model
        .table(id)
        .ok_or_else(|| CompileError::Field(format!("a field refers to a missing table ({id})")))
}

fn number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{n:.0}")
    } else {
        format!("{n}")
    }
}

/// Whether a row expression can be evaluated inside an aggregate (no measures,
/// aggregates, CALCULATE, context facts or variables).
fn row_pure(b: &Bound) -> bool {
    let mut ok = true;
    charted_dax::bind::walk(b, &mut |x| {
        if matches!(
            x,
            Bound::Measure { .. }
                | Bound::Aggregate { .. }
                | Bound::Rank { .. }
                | Bound::ConcatX { .. }
                | Bound::Calculate { .. }
                | Bound::Info { .. }
                | Bound::Var(_)
                | Bound::Let { .. }
        ) {
            ok = false;
        }
    });
    ok
}

/// ALLSELECTED as a table reads VALUES (or the table) under the context without its
/// grouping keys; other sources are read as they are.
fn unselect(source: &TableExpr, ctx: &Ctx) -> Option<(TableExpr, Ctx)> {
    let TableExpr::AllSelected { table, columns } = source else {
        return None;
    };
    let t = if columns.is_empty() {
        TableExpr::Table(table.clone())
    } else {
        TableExpr::Values {
            table: table.clone(),
            columns: columns.clone(),
        }
    };
    Some((
        t,
        Ctx {
            keys: Vec::new(),
            ..ctx.clone()
        },
    ))
}

/// A row expression over the rows of `table` alone: columns of other tables are an
/// outer row's values, which only a frame has.
fn row_local(b: &Bound, table: &str) -> bool {
    let mut ok = row_pure(b);
    charted_dax::bind::walk(b, &mut |x| {
        if let Bound::Column { table: t, .. } = x
            && t != table
        {
            ok = false;
        }
    });
    ok
}

/// RELATED lookups in an expression, not counting those inside nested row contexts.
fn related_refs(b: &Bound, out: &mut Vec<(Id, Id)>) {
    match b {
        Bound::Related { table, column } => {
            if !out.contains(&(table.clone(), column.clone())) {
                out.push((table.clone(), column.clone()));
            }
        }
        Bound::Aggregate { .. } | Bound::Calculate { .. } | Bound::Measure { .. } => {}
        Bound::Let { value, body, .. } => {
            related_refs(value, out);
            related_refs(body, out);
        }
        Bound::Call { args, .. } => args.iter().for_each(|a| related_refs(a, out)),
        Bound::Info {
            alternate: Some(a), ..
        } => related_refs(a, out),
        Bound::Switch {
            value,
            cases,
            default,
        } => {
            related_refs(value, out);
            for (w, t) in cases {
                related_refs(w, out);
                related_refs(t, out);
            }
            if let Some(d) = default {
                related_refs(d, out);
            }
        }
        Bound::In { value, list } => {
            related_refs(value, out);
            list.iter().for_each(|a| related_refs(a, out));
        }
        Bound::Unary { expr, .. } => related_refs(expr, out),
        Bound::Binary { left, right, .. } => {
            related_refs(left, out);
            related_refs(right, out);
        }
        _ => {}
    }
}

fn agg_sql(func: AggFunc, arg: Option<(String, ValueType)>) -> (String, ValueType) {
    let (x, ty) = arg.unwrap_or(("NULL".into(), ValueType::Number));
    match func {
        AggFunc::Sum => (format!("sum({x})"), ValueType::Number),
        AggFunc::Average => (format!("avg({x})"), ValueType::Number),
        AggFunc::Min => (format!("min({x})"), ty),
        AggFunc::Max => (format!("max({x})"), ty),
        // Counting nothing is blank in DAX.
        AggFunc::Count => (format!("nullif(count({x}), 0)"), ValueType::Number),
        AggFunc::CountRows => ("nullif(count(*), 0)".into(), ValueType::Number),
        AggFunc::CountBlank => (
            format!("nullif(count(*) FILTER (WHERE ({x}) IS NULL), 0)"),
            ValueType::Number,
        ),
        // DISTINCTCOUNT counts blank as a value.
        AggFunc::DistinctCount => (
            format!("nullif(count(DISTINCT {x}) + coalesce(max((({x}) IS NULL)::INTEGER), 0), 0)"),
            ValueType::Number,
        ),
    }
}

/// Casts one side to text when a CASE or COALESCE mixes text with other types.
fn unify(parts: &[(String, ValueType)]) -> (Vec<String>, ValueType) {
    let kinds: Vec<ValueType> = parts
        .iter()
        .filter(|(s, _)| s != "NULL")
        .map(|(_, t)| *t)
        .collect();
    let ty = kinds.first().copied().unwrap_or(ValueType::Number);
    if kinds.iter().all(|t| *t == ty) {
        return (parts.iter().map(|(s, _)| s.clone()).collect(), ty);
    }
    (
        parts
            .iter()
            .map(|(s, _)| format!("CAST({s} AS VARCHAR)"))
            .collect(),
        ValueType::Text,
    )
}

/// A value used as a condition: numbers are true when non-zero, as in DAX.
fn bool_of(x: &(String, ValueType)) -> String {
    match x.1 {
        ValueType::Bool => x.0.clone(),
        ValueType::Number => format!("(({}) <> 0)", x.0),
        _ => format!("(({}) IS NOT NULL)", x.0),
    }
}

/// Blank compared as zero, empty text or false.
fn blank_as(x: &(String, ValueType), like: ValueType) -> String {
    match like {
        ValueType::Number => format!("coalesce({}, 0)", x.0),
        ValueType::Text => format!("coalesce(CAST({} AS VARCHAR), '')", x.0),
        ValueType::Bool => format!("coalesce({}, FALSE)", x.0),
        ValueType::Date => x.0.clone(),
    }
}

fn eq_sql(a: &(String, ValueType), b: &(String, ValueType)) -> String {
    let like = if a.1 == ValueType::Text || b.1 == ValueType::Text {
        ValueType::Text
    } else {
        a.1
    };
    format!("({} = {})", blank_as(a, like), blank_as(b, like))
}

/// A value as text, as the interpreter's `Value::key` writes it (whole numbers without
/// decimals), or NULL when it is blank.
fn text_or_null(x: &(String, ValueType)) -> String {
    match x.1 {
        ValueType::Text => x.0.clone(),
        ValueType::Number => format!(
            "(CASE WHEN {d} = trunc({d}) AND abs({d}) < 1e15 THEN CAST(CAST({d} AS BIGINT) AS VARCHAR) ELSE CAST({d} AS VARCHAR) END)",
            d = format!("CAST({} AS DOUBLE)", x.0)
        ),
        ValueType::Bool | ValueType::Date => format!("CAST({} AS VARCHAR)", x.0),
    }
}

/// A text argument of the text functions: blank is empty text.
fn text_sql(x: &(String, ValueType)) -> String {
    format!("coalesce({}, '')", text_or_null(x))
}

/// A whole-number argument: blank counts as zero, decimals are truncated.
fn whole_sql(x: &(String, ValueType)) -> String {
    format!(
        "CAST(trunc(coalesce(CAST({} AS DOUBLE), 0)) AS BIGINT)",
        x.0
    )
}

/// A date argument (bad text is blank, as in the interpreter).
fn date_sql(x: &(String, ValueType)) -> String {
    format!("TRY_CAST({} AS DATE)", x.0)
}

/// FORMAT's text for a value, or an error when the value and the format don't match.
fn format_sql(spec: &FormatSpec, x: &(String, ValueType)) -> Result<String> {
    let body = match (spec, x.1) {
        (FormatSpec::General, ValueType::Number) => text_or_null(x),
        (
            FormatSpec::Number {
                decimals,
                thousands,
                percent,
                prefix,
            },
            ValueType::Number,
        ) => {
            // Rounded first (halves away from zero, like the interpreter), then written
            // without the sign, which goes before the currency symbol.
            let scaled = if *percent {
                format!("CAST({} AS DOUBLE) * 100", x.0)
            } else {
                format!("CAST({} AS DOUBLE)", x.0)
            };
            let r = format!("round({scaled}, {decimals})");
            let digits = format!(
                "format('{{:{}.{decimals}f}}', abs({r}))",
                if *thousands { "," } else { "" }
            );
            format!(
                "(CASE WHEN {r} < 0 THEN '-' ELSE '' END || {} || {digits} || {})",
                string(prefix),
                string(if *percent { "%" } else { "" })
            )
        }
        (FormatSpec::Date(parts), ValueType::Date | ValueType::Text) => {
            let pattern: String = parts
                .iter()
                .map(|p| match p {
                    DatePart::Year => "%Y".to_string(),
                    DatePart::Year2 => "%y".into(),
                    DatePart::MonthName => "%B".into(),
                    DatePart::MonthAbbr => "%b".into(),
                    DatePart::Month2 => "%m".into(),
                    DatePart::Month => "%-m".into(),
                    DatePart::DayName => "%A".into(),
                    DatePart::DayAbbr => "%a".into(),
                    DatePart::Day2 => "%d".into(),
                    DatePart::Day => "%-d".into(),
                    DatePart::Literal(l) => l.replace('%', "%%"),
                })
                .collect();
            format!("strftime({}, {})", date_sql(x), string(&pattern))
        }
        (FormatSpec::Date(_), _) => {
            return Err(CompileError::Unsupported(
                "FORMAT with a date format string of a value that isn't a date".into(),
            ));
        }
        _ => {
            return Err(CompileError::Unsupported(
                "FORMAT with a number format string of a value that isn't a number".into(),
            ));
        }
    };
    Ok(format!("coalesce({body}, '')"))
}

fn call_sql(func: &ScalarFunc, a: &[(String, ValueType)]) -> Result<(String, ValueType)> {
    let num = ValueType::Number;
    Ok(match func {
        ScalarFunc::Divide => {
            let alt = a.get(2).map(|x| x.0.as_str()).unwrap_or("NULL");
            (
                format!(
                    "(CASE WHEN coalesce({b}, 0) = 0 THEN {alt} ELSE ({x}) / ({b}) END)",
                    x = a[0].0,
                    b = a[1].0
                ),
                num,
            )
        }
        ScalarFunc::If => {
            let mut branches = vec![a[1].clone()];
            branches.push(a.get(2).cloned().unwrap_or(("NULL".into(), a[1].1)));
            let (b, ty) = unify(&branches);
            (
                format!(
                    "(CASE WHEN coalesce({}, FALSE) THEN {} ELSE {} END)",
                    bool_of(&a[0]),
                    b[0],
                    b[1]
                ),
                ty,
            )
        }
        ScalarFunc::Abs => (format!("abs({})", a[0].0), num),
        // DAX rounds halves away from zero.
        ScalarFunc::Round => (
            format!(
                "round(CAST({} AS DOUBLE), CAST({} AS INTEGER))",
                a[0].0, a[1].0
            ),
            num,
        ),
        ScalarFunc::Coalesce => {
            let (b, ty) = unify(a);
            (format!("coalesce({})", b.join(", ")), ty)
        }
        ScalarFunc::And => (
            format!(
                "(coalesce({}, FALSE) AND coalesce({}, FALSE))",
                bool_of(&a[0]),
                bool_of(&a[1])
            ),
            ValueType::Bool,
        ),
        ScalarFunc::Or => (
            format!(
                "(coalesce({}, FALSE) OR coalesce({}, FALSE))",
                bool_of(&a[0]),
                bool_of(&a[1])
            ),
            ValueType::Bool,
        ),
        ScalarFunc::Not => (
            format!("(NOT coalesce({}, FALSE))", bool_of(&a[0])),
            ValueType::Bool,
        ),
        ScalarFunc::IsBlank => (format!("(({}) IS NULL)", a[0].0), ValueType::Bool),
        ScalarFunc::Year | ScalarFunc::Month | ScalarFunc::Day | ScalarFunc::Quarter => {
            let part = match func {
                ScalarFunc::Year => "year",
                ScalarFunc::Month => "month",
                ScalarFunc::Day => "day",
                _ => "quarter",
            };
            (
                format!("CAST({part}(TRY_CAST({} AS TIMESTAMP)) AS BIGINT)", a[0].0),
                num,
            )
        }
        // TRUE and FALSE are 1 and 0 (INT ( x > 0 ) is a common idiom).
        ScalarFunc::Int => (format!("floor(CAST({} AS DOUBLE))", a[0].0), num),
        ScalarFunc::Min | ScalarFunc::Max => {
            let f = if matches!(func, ScalarFunc::Min) {
                "least"
            } else {
                "greatest"
            };
            let numeric = |t: ValueType| matches!(t, ValueType::Number | ValueType::Bool);
            if numeric(a[0].1) && numeric(a[1].1) {
                // Numbers: blank counts as zero unless both are blank.
                (
                    format!(
                        "(CASE WHEN ({x}) IS NULL AND ({y}) IS NULL THEN NULL ELSE {f}(coalesce(CAST({x} AS DOUBLE), 0), coalesce(CAST({y} AS DOUBLE), 0)) END)",
                        x = a[0].0,
                        y = a[1].0
                    ),
                    num,
                )
            } else {
                // Text and dates: least/greatest ignore the blank one.
                let (p, ty) = unify(a);
                (format!("{f}({}, {})", p[0], p[1]), ty)
            }
        }
        ScalarFunc::Upper => (format!("upper({})", text_sql(&a[0])), ValueType::Text),
        ScalarFunc::Lower => (format!("lower({})", text_sql(&a[0])), ValueType::Text),
        ScalarFunc::Len => (format!("length({})", text_sql(&a[0])), num),
        ScalarFunc::Trim => (
            format!("trim(regexp_replace({}, ' +', ' ', 'g'))", text_sql(&a[0])),
            ValueType::Text,
        ),
        ScalarFunc::Left | ScalarFunc::Right => {
            let n = a.get(1).map(whole_sql).unwrap_or_else(|| "1".into());
            let f = if matches!(func, ScalarFunc::Left) {
                "left"
            } else {
                "right"
            };
            (
                format!("{f}({}, greatest({n}, 0))", text_sql(&a[0])),
                ValueType::Text,
            )
        }
        ScalarFunc::Mid => (
            format!(
                "substring({}, greatest({}, 1), greatest({}, 0))",
                text_sql(&a[0]),
                whole_sql(&a[1]),
                whole_sql(&a[2])
            ),
            ValueType::Text,
        ),
        ScalarFunc::Substitute => {
            let (t, old, new) = (text_sql(&a[0]), text_sql(&a[1]), text_sql(&a[2]));
            let sql = match a.get(3) {
                None => {
                    format!("(CASE WHEN {old} = '' THEN {t} ELSE replace({t}, {old}, {new}) END)")
                }
                // Only the k-th occurrence: the parts around it, joined back.
                Some(k) => {
                    let k = whole_sql(k);
                    let parts = format!("string_split({t}, {old})");
                    format!(
                        "(CASE WHEN {old} = '' OR {k} < 1 OR {k} >= len({parts}) THEN {t} ELSE array_to_string(list_slice({parts}, 1, {k}), {old}) || {new} || array_to_string(list_slice({parts}, {k} + 1, len({parts})), {old}) END)"
                    )
                }
            };
            (sql, ValueType::Text)
        }
        ScalarFunc::Format(spec) => (format_sql(spec, &a[0])?, ValueType::Text),
        ScalarFunc::Date => {
            let y = whole_sql(&a[0]);
            (
                format!(
                    "CAST(make_date(CASE WHEN {y} BETWEEN 0 AND 1899 THEN {y} + 1900 ELSE {y} END, 1, 1) + to_months(CAST({} - 1 AS INTEGER)) + to_days(CAST({} - 1 AS INTEGER)) AS DATE)",
                    whole_sql(&a[1]),
                    whole_sql(&a[2])
                ),
                ValueType::Date,
            )
        }
        ScalarFunc::Today => ("current_date".into(), ValueType::Date),
        ScalarFunc::EoMonth => (
            format!(
                "last_day(CAST(date_trunc('month', {}) + to_months(CAST({} AS INTEGER)) AS DATE))",
                date_sql(&a[0]),
                whole_sql(&a[1])
            ),
            ValueType::Date,
        ),
        ScalarFunc::EDate => (
            format!(
                "CAST({} + to_months(CAST({} AS INTEGER)) AS DATE)",
                date_sql(&a[0]),
                whole_sql(&a[1])
            ),
            ValueType::Date,
        ),
        // date_diff counts the unit boundaries crossed, as DATEDIFF does.
        ScalarFunc::DateDiff(p) => (
            format!(
                "date_diff('{}', {}, {})",
                p.sql(),
                date_sql(&a[0]),
                date_sql(&a[1])
            ),
            num,
        ),
    })
}

fn unary_sql(op: UnaryOp, x: &(String, ValueType)) -> (String, ValueType) {
    match op {
        UnaryOp::Neg => (format!("(-({}))", x.0), ValueType::Number),
        UnaryOp::Plus => x.clone(),
        UnaryOp::Not => (
            format!("(NOT coalesce({}, FALSE))", bool_of(x)),
            ValueType::Bool,
        ),
    }
}

fn binary_sql(
    op: BinaryOp,
    l: &(String, ValueType),
    r: &(String, ValueType),
) -> (String, ValueType) {
    let (a, b) = (&l.0, &r.0);
    let like = if l.1 == ValueType::Text || r.1 == ValueType::Text {
        ValueType::Text
    } else if l.1 == ValueType::Date || r.1 == ValueType::Date {
        ValueType::Date
    } else {
        l.1
    };
    let cmp = |sym: &str| {
        (
            format!("({} {sym} {})", blank_as(l, like), blank_as(r, like)),
            ValueType::Bool,
        )
    };
    match op {
        BinaryOp::Add | BinaryOp::Sub => {
            let sym = if op == BinaryOp::Add { "+" } else { "-" };
            (
                format!(
                    "(CASE WHEN ({a}) IS NULL AND ({b}) IS NULL THEN NULL ELSE coalesce({a}, 0) {sym} coalesce({b}, 0) END)"
                ),
                ValueType::Number,
            )
        }
        BinaryOp::Mul => (format!("(({a}) * ({b}))"), ValueType::Number),
        BinaryOp::Div => (format!("(({a}) / nullif({b}, 0))"), ValueType::Number),
        BinaryOp::Pow => (format!("power({a}, {b})"), ValueType::Number),
        BinaryOp::Concat => (
            format!("(coalesce(CAST({a} AS VARCHAR), '') || coalesce(CAST({b} AS VARCHAR), ''))"),
            ValueType::Text,
        ),
        BinaryOp::Eq => cmp("="),
        BinaryOp::StrictEq => (
            format!("(({a}) IS NOT DISTINCT FROM ({b}))"),
            ValueType::Bool,
        ),
        BinaryOp::Ne => cmp("<>"),
        BinaryOp::Lt => cmp("<"),
        BinaryOp::Le => cmp("<="),
        BinaryOp::Gt => cmp(">"),
        BinaryOp::Ge => cmp(">="),
        BinaryOp::And => (
            format!(
                "(coalesce({}, FALSE) AND coalesce({}, FALSE))",
                bool_of(l),
                bool_of(r)
            ),
            ValueType::Bool,
        ),
        BinaryOp::Or => (
            format!(
                "(coalesce({}, FALSE) OR coalesce({}, FALSE))",
                bool_of(l),
                bool_of(r)
            ),
            ValueType::Bool,
        ),
        BinaryOp::In => ("NULL".into(), ValueType::Bool),
    }
}

/// A literal compared against a value of type `t`.
fn typed_literal(text: &str, t: DataType) -> String {
    match crate::sqltype(t) {
        Some(target) if t != DataType::String => {
            format!("TRY_CAST({} AS {target})", string(text.trim()))
        }
        _ => string(text),
    }
}

/// The predicate for a column filter, over `e` (a column of type `t`).
fn predicate(e: &str, t: DataType, kind: &FilterKind) -> Option<String> {
    let text = format!("CAST({e} AS VARCHAR)");
    match kind {
        FilterKind::All | FilterKind::TopN { .. } => None,
        FilterKind::Basic { values, exclude } => {
            let listed: Vec<String> = values.iter().flatten().map(|v| string(v)).collect();
            let mut parts = Vec::new();
            if !listed.is_empty() {
                parts.push(format!("{text} IN ({})", listed.join(", ")));
            }
            if values.iter().any(Option::is_none) {
                parts.push(format!("{e} IS NULL"));
            }
            let any = if parts.is_empty() {
                "FALSE".to_string()
            } else {
                format!("({})", parts.join(" OR "))
            };
            Some(if *exclude {
                format!("NOT coalesce({any}, FALSE)")
            } else {
                any
            })
        }
        FilterKind::Advanced { conditions, or } => {
            let parts: Vec<String> = conditions
                .iter()
                .filter_map(|c| condition(e, t, c))
                .collect();
            if parts.is_empty() {
                return None;
            }
            Some(format!(
                "({})",
                parts.join(if *or { " OR " } else { " AND " })
            ))
        }
        FilterKind::RelativeDate {
            period,
            count,
            unit,
        } => {
            let (lo, hi) = relative_range(*period, *count, *unit);
            Some(format!(
                "(CAST({e} AS DATE) >= {lo} AND CAST({e} AS DATE) < {hi})"
            ))
        }
    }
}

/// The half-open date range `[lo, hi)` of a relative date filter, as SQL over
/// `current_date`. "Last 3 months" runs from the day after the same date three months
/// ago through today; "this month" is the calendar month; "next" starts tomorrow.
pub fn relative_range(period: RelativePeriod, count: u32, unit: DateUnit) -> (String, String) {
    let n = count.max(1);
    let unit_sql = match unit {
        DateUnit::Days => "day",
        DateUnit::Weeks => "week",
        DateUnit::Months => "month",
        DateUnit::Years => "year",
    };
    let today = "current_date";
    let tomorrow = "CAST(current_date + INTERVAL 1 day AS DATE)";
    match period {
        RelativePeriod::Last => (
            format!("CAST({tomorrow} - INTERVAL {n} {unit_sql} AS DATE)"),
            tomorrow.to_string(),
        ),
        RelativePeriod::Next => (
            tomorrow.to_string(),
            format!("CAST({tomorrow} + INTERVAL {n} {unit_sql} AS DATE)"),
        ),
        RelativePeriod::This => {
            let start = format!("CAST(date_trunc('{unit_sql}', {today}) AS DATE)");
            let end = if unit == DateUnit::Days {
                tomorrow.to_string()
            } else {
                format!("CAST({start} + INTERVAL 1 {unit_sql} AS DATE)")
            };
            (start, end)
        }
    }
}

fn condition(e: &str, t: DataType, c: &Condition) -> Option<String> {
    let text = format!("lower(CAST({e} AS VARCHAR))");
    let lit = |v: &Option<String>| v.as_deref().map(|s| typed_literal(s, t));
    let low = |v: &Option<String>| v.as_deref().map(|s| format!("lower({})", string(s)));
    let is_text = t == DataType::String;
    Some(match c.op {
        FilterOp::IsBlank => format!("({e} IS NULL OR CAST({e} AS VARCHAR) = '')"),
        FilterOp::IsNotBlank => format!("({e} IS NOT NULL AND CAST({e} AS VARCHAR) <> '')"),
        FilterOp::Contains => format!("contains({text}, {})", low(&c.value)?),
        FilterOp::NotContains => {
            format!("NOT coalesce(contains({text}, {}), FALSE)", low(&c.value)?)
        }
        FilterOp::StartsWith => format!("starts_with({text}, {})", low(&c.value)?),
        FilterOp::EndsWith => format!("ends_with({text}, {})", low(&c.value)?),
        FilterOp::Equals if is_text => format!("{text} = {}", low(&c.value)?),
        FilterOp::NotEquals if is_text => format!("{text} IS DISTINCT FROM {}", low(&c.value)?),
        FilterOp::Between => format!("{e} BETWEEN {} AND {}", lit(&c.value)?, lit(&c.value2)?),
        op => {
            let sym = match op {
                FilterOp::Equals => "=",
                FilterOp::NotEquals => "IS DISTINCT FROM",
                FilterOp::Less => "<",
                FilterOp::LessOrEqual => "<=",
                FilterOp::Greater => ">",
                _ => ">=",
            };
            format!("{e} {sym} {}", lit(&c.value)?)
        }
    })
}

impl<'a> Compiler<'a> {
    fn fresh(&mut self, prefix: &str) -> String {
        self.counter += 1;
        format!("{prefix}{}", self.counter)
    }

    /// A column's value in its model type, over its table's alias.
    fn column_sql(&self, table_id: &str, column: &str) -> Result<(String, ValueType)> {
        let t = table(self.model, table_id)?;
        let c = t.column(column).ok_or_else(|| {
            CompileError::Field(format!(
                "a field refers to a missing column of '{}'",
                t.name
            ))
        })?;
        let source_column = match &c.kind {
            ColumnKind::Data { source_column } => source_column.clone(),
            // Calculated columns are materialised next to the loaded ones.
            ColumnKind::Calculated { .. } => crate::calc_column_name(&c.id),
        };
        let raw = format!("{}.{}", alias(self.model, table_id), ident(&source_column));
        let physical = self.catalog.physical_type(&t.source, &source_column);
        if physical.is_none() && matches!(c.kind, ColumnKind::Calculated { .. }) {
            return Err(CompileError::Field(format!(
                "the calculated column '{}'[{}] hasn't been computed yet",
                t.name, c.name
            )));
        }
        let sql = match (physical, crate::sqltype(c.data_type)) {
            (Some(p), Some(target)) if p != c.data_type => format!("TRY_CAST({raw} AS {target})"),
            _ => raw,
        };
        Ok((sql, ValueType::of(c.data_type)))
    }

    fn column_type(&self, table_id: &str, column: &str) -> DataType {
        self.model
            .table(table_id)
            .and_then(|t| t.column(column))
            .map(|c| c.data_type)
            .unwrap_or(DataType::String)
    }

    /// SQL for a filter's condition over the aliases of the tables of a CTE.
    fn pred_sql(&mut self, f: &CFilter) -> Result<Option<String>> {
        Ok(match &f.pred {
            Pred::Cond(b) => Some(bool_of(&self.row(b, None)?)),
            Pred::Kind(col, kind) => {
                let (e, _) = self.column_sql(&f.table, col)?;
                predicate(&e, self.column_type(&f.table, col), kind)
            }
            Pred::Sql(col, template) => {
                let (e, _) = self.column_sql(&f.table, col)?;
                Some(template.replace('\u{7}', &e))
            }
            Pred::InSet { set, columns, keys } => {
                let mut conds = Vec::new();
                for (i, c) in columns.iter().enumerate() {
                    conds.push(format!(
                        "s.v{i} IS NOT DISTINCT FROM {}",
                        self.column_sql(&f.table, c)?.0
                    ));
                }
                for k in keys {
                    conds.push(format!(
                        "s.{} IS NOT DISTINCT FROM {}",
                        k.name,
                        self.column_sql(&k.table, &k.column)?.0
                    ));
                }
                Some(format!(
                    "EXISTS (SELECT 1 FROM {} AS s WHERE {})",
                    self.ctes[*set].name,
                    conds.join(" AND ")
                ))
            }
        })
    }

    /// Tables a filter's condition needs joined (its table, and RELATED lookups).
    fn pred_tables(f: &CFilter) -> Vec<Id> {
        let mut out = vec![f.table.clone()];
        match &f.pred {
            Pred::Cond(b) => {
                let mut rel = Vec::new();
                related_refs(b, &mut rel);
                out.extend(rel.into_iter().map(|(t, _)| t));
            }
            Pred::InSet { keys, .. } => out.extend(keys.iter().map(|k| k.table.clone())),
            _ => {}
        }
        out
    }

    /// `source AS alias` for `base`, left-joined to every table in `needed` along its path.
    fn joins(&self, reach: &Reach, base: &str, needed: &[Id]) -> Result<(String, bool)> {
        let model = self.model;
        let src = |id: &str| -> Result<String> {
            Ok(format!(
                "{} AS {}",
                ident(&table(model, id)?.source),
                alias(model, id)
            ))
        };
        let mut joined: Vec<Id> = vec![base.to_string()];
        let mut sql = src(base)?;
        let mut expands = false;
        for t in needed {
            if reach.is_ambiguous(t) {
                return Err(CompileError::Ambiguous {
                    from: table(model, base)?.name.clone(),
                    to: table(model, t)?.name.clone(),
                });
            }
            for hop in reach.path(t) {
                if joined.contains(&hop.to) {
                    continue;
                }
                let col =
                    |tid: &str, cid: &str| -> Result<String> { Ok(self.column_sql(tid, cid)?.0) };
                sql.push_str(&format!(
                    " LEFT JOIN {} ON {} = {}",
                    src(&hop.to)?,
                    col(&hop.from, &hop.from_column)?,
                    col(&hop.to, &hop.to_column)?
                ));
                joined.push(hop.to.clone());
                expands |= hop.expands;
            }
        }
        Ok((sql, expands))
    }

    /// SQL that is true when another table refers to keys `t` lacks: `t` then has a blank
    /// member (DAX's blank row).
    fn blank_exists(&self, t: &str) -> Result<Option<String>> {
        let mut exists = Vec::new();
        for r in self
            .model
            .relationships
            .iter()
            .filter(|r| r.active && r.to_table == t)
        {
            let (fk, _) = self.column_sql(&r.from_table, &r.from_column)?;
            let (pk, _) = self.column_sql(&r.to_table, &r.to_column)?;
            exists.push(format!(
                "EXISTS (SELECT 1 FROM {} AS {} WHERE {fk} IS NOT NULL AND NOT EXISTS (SELECT 1 FROM {} AS {} WHERE {pk} = {fk}))",
                ident(&table(self.model, &r.from_table)?.source),
                alias(self.model, &r.from_table),
                ident(&table(self.model, t)?.source),
                alias(self.model, t),
            ));
        }
        Ok((!exists.is_empty()).then(|| format!("({})", exists.join(" OR "))))
    }

    /// A FROM clause whose base table contributes only its blank member (an all-null row).
    fn blank_from(&self, from: &str, base: &str) -> Result<String> {
        let t = table(self.model, base)?;
        let head = format!("{} AS {}", ident(&t.source), alias(self.model, base));
        Ok(from.replacen(
            &head,
            &format!("(SELECT 1) AS one LEFT JOIN {head} ON FALSE"),
            1,
        ))
    }

    /// `SELECT DISTINCT cols FROM from WHERE preds`, plus the blank member's row when the
    /// table has one and it passes `preds`.
    fn distinct_with_blank(
        &self,
        cols: &str,
        base: &str,
        from: &str,
        preds: &[String],
    ) -> Result<String> {
        let wh = if preds.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", preds.join(" AND "))
        };
        let mut sql = format!("SELECT DISTINCT {cols} FROM {from}{wh}");
        if let Some(exists) = self.blank_exists(base)? {
            let mut p = preds.to_vec();
            p.push(exists);
            sql.push_str(&format!(
                " UNION SELECT DISTINCT {cols} FROM {} WHERE {}",
                self.blank_from(from, base)?,
                p.join(" AND ")
            ));
        }
        Ok(sql)
    }

    /// The grouping keys and filters of `ctx` that reach `base`, and its FROM clause.
    /// The grouping keys (with their SQL) and filters of `ctx` that reach `base`, and
    /// its FROM clause. Value sets computed per key are joined, and supply those keys.
    fn scope(
        &mut self,
        base: &str,
        ctx: &Ctx,
        extra_tables: &[Id],
        apply_ctx: bool,
    ) -> Result<Scope> {
        let reach = Reach::with(self.model, base, &ctx.use_rels);
        let keys: Vec<Key> = if apply_ctx {
            ctx.keys
                .iter()
                .filter(|k| reach.reaches(base, &k.table))
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        let filters: Vec<CFilter> = if apply_ctx {
            ctx.filters
                .iter()
                .filter(|f| reach.reaches(base, &f.table))
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        let mut needed: Vec<Id> = keys.iter().map(|k| k.table.clone()).collect();
        for f in &filters {
            // A value set's keys from tables out of reach are carried, not joined: the
            // aggregate is computed per set key instead.
            needed.extend(
                Self::pred_tables(f)
                    .into_iter()
                    .filter(|t| t == &f.table || t == base || reach.reaches(base, t)),
            );
        }
        needed.extend(extra_tables.iter().cloned());
        for t in &needed {
            if t != base && !reach.reaches(base, t) {
                return Err(CompileError::Field(format!(
                    "'{}' isn't related to '{}'",
                    table(self.model, t)?.name,
                    table(self.model, base)?.name
                )));
            }
        }
        let (mut from, expands) = self.joins(&reach, base, &needed)?;
        let mut out_keys: Vec<(Key, String)> = keys
            .iter()
            .map(|k| Ok((k.clone(), self.column_sql(&k.table, &k.column)?.0)))
            .collect::<Result<_>>()?;
        let mut preds = Vec::new();
        for f in &filters {
            if let Pred::InSet {
                set,
                columns,
                keys: set_keys,
            } = &f.pred
                && !set_keys.is_empty()
            {
                // Joined: each row counts for every key whose set holds its values.
                let s = self.fresh("vs");
                let mut on = Vec::new();
                for (i, c) in columns.iter().enumerate() {
                    on.push(format!(
                        "{s}.v{i} IS NOT DISTINCT FROM {}",
                        self.column_sql(&f.table, c)?.0
                    ));
                }
                for k in set_keys {
                    match out_keys.iter().find(|(x, _)| x.name == k.name) {
                        Some((_, e)) => on.push(format!("{s}.{} IS NOT DISTINCT FROM {e}", k.name)),
                        None => out_keys.push((k.clone(), format!("{s}.{}", k.name))),
                    }
                }
                from.push_str(&format!(
                    " JOIN {} AS {s} ON {}",
                    self.ctes[*set].name,
                    on.join(" AND ")
                ));
                continue;
            }
            if let Some(p) = self.pred_sql(f)? {
                preds.push(p);
            }
        }
        Ok((out_keys, preds, from, expands))
    }

    fn add_cte(&mut self, sql: String, keys: Vec<String>, exact: bool) -> usize {
        if let Some(i) = self
            .ctes
            .iter()
            .position(|c| c.sql == sql && c.keys == keys)
        {
            return i;
        }
        let name = format!("c{}", self.ctes.len());
        self.ctes.push(CteDef {
            name,
            sql,
            keys,
            exact,
        });
        self.ctes.len() - 1
    }

    /// Whether a context is the visual's own (same filters, no relationship changes).
    fn is_visual_ctx(&self, ctx: &Ctx) -> bool {
        ctx.use_rels.is_empty() && ctx.filters == self.top_filters
    }

    /// An aggregate of a row expression over a table's rows, as a CTE value.
    fn simple_aggregate(
        &mut self,
        func: AggFunc,
        source: &TableExpr,
        arg: Option<&Bound>,
        ctx: &Ctx,
    ) -> Result<Scalar> {
        // Unwrap the source: FILTERs with row conditions, VALUES/ALL with columns.
        let mut conds: Vec<&Bound> = Vec::new();
        let mut cur = source;
        while let TableExpr::Filter { source, condition } = cur {
            conds.push(condition);
            cur = source;
        }
        let selected = unselect(cur, ctx);
        let (cur, ctx) = match &selected {
            Some((t, c)) => (t, c),
            None => (cur, ctx),
        };
        let (base, distinct, apply_ctx) = match cur {
            TableExpr::Table(t) => (t.clone(), None, true),
            TableExpr::Values { table, columns } => (table.clone(), Some(columns.clone()), true),
            TableExpr::All { table, columns } => (
                table.clone(),
                if columns.is_empty() {
                    None
                } else {
                    Some(columns.clone())
                },
                false,
            ),
            TableExpr::List(_) => {
                return Err(CompileError::Unsupported(
                    "iterating a list of values".into(),
                ));
            }
            TableExpr::Dates { .. } => {
                return Err(CompileError::Unsupported(
                    "iterating a time-intelligence table (use it as a CALCULATE filter)".into(),
                ));
            }
            TableExpr::Filter { .. } | TableExpr::AllSelected { .. } | TableExpr::TopN { .. } => {
                unreachable!("filters and ALLSELECTED were unwrapped; TOPN takes the frame path")
            }
        };
        let mut related: Vec<(Id, Id)> = Vec::new();
        if let Some(a) = arg {
            related_refs(a, &mut related);
        }
        for c in &conds {
            related_refs(c, &mut related);
        }
        let extra: Vec<Id> = related.iter().map(|(t, _)| t.clone()).collect();
        let (keys, mut preds, from, expands) = self.scope(&base, ctx, &extra, apply_ctx)?;
        for c in &conds {
            preds.push(bool_of(&self.row(c, None)?));
        }
        let key_exprs: Vec<(String, String)> = keys
            .iter()
            .map(|(k, e)| (e.clone(), k.name.clone()))
            .collect();
        let wh = if preds.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", preds.join(" AND "))
        };
        let key_names: Vec<String> = keys.iter().map(|(k, _)| k.name.clone()).collect();
        let sql = match distinct {
            // Joins that can repeat the base rows: each base row counts once per key.
            None if expands => {
                let mut inner: Vec<String> = key_exprs
                    .iter()
                    .map(|(e, n)| format!("{e} AS {n}"))
                    .collect();
                inner.push(format!("{}.rowid AS rid", alias(self.model, &base)));
                let row = match arg {
                    Some(a) => {
                        let (x, ty) = self.row(a, None)?;
                        inner.push(format!("{x} AS a"));
                        Some(("a".to_string(), ty))
                    }
                    None => None,
                };
                let (agg, _) = agg_sql(func, row);
                let mut sql = format!(
                    "SELECT {}{agg} AS v FROM (SELECT DISTINCT {} FROM {from}{wh}) AS t",
                    key_names
                        .iter()
                        .map(|n| format!("{n}, "))
                        .collect::<String>(),
                    inner.join(", ")
                );
                if !keys.is_empty() {
                    sql.push_str(&format!(" GROUP BY {}", key_names.join(", ")));
                }
                sql
            }
            None => {
                let row = match arg {
                    Some(a) => Some(self.row(a, None)?),
                    None => None,
                };
                let (agg, _) = agg_sql(func, row);
                let mut select: Vec<String> = key_exprs
                    .iter()
                    .map(|(e, n)| format!("{e} AS {n}"))
                    .collect();
                select.push(format!("{agg} AS v"));
                let mut sql = format!("SELECT {} FROM {from}{wh}", select.join(", "));
                if !keys.is_empty() {
                    sql.push_str(&format!(
                        " GROUP BY {}",
                        (1..=keys.len())
                            .map(|i| i.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                sql
            }
            Some(cols) => {
                // Distinct rows of some columns, then the aggregate over them.
                let mut inner: Vec<String> = key_exprs
                    .iter()
                    .map(|(e, n)| format!("{e} AS {n}"))
                    .collect();
                let mut names = Vec::new();
                for (i, c) in cols.iter().enumerate() {
                    inner.push(format!("{} AS d{i}", self.column_sql(&base, c)?.0));
                    names.push((c.clone(), format!("d{i}")));
                }
                let row = match arg {
                    Some(a) => Some(self.row(a, Some(&names))?),
                    None => None,
                };
                let (agg, _) = agg_sql(func, row);
                let rows = self.distinct_with_blank(&inner.join(", "), &base, &from, &preds)?;
                let mut sql = format!(
                    "SELECT {}{agg} AS v FROM ({rows}) AS t",
                    key_names
                        .iter()
                        .map(|n| format!("{n}, "))
                        .collect::<String>(),
                );
                if !keys.is_empty() {
                    sql.push_str(&format!(" GROUP BY {}", key_names.join(", ")));
                }
                sql
            }
        };
        // Joins across a Both relationship from its one side give rows without a match
        // blank keys, which the visual may not have: check those against the domains.
        let exact = self.is_visual_ctx(ctx) && !expands;
        let i = self.add_cte(sql, key_names, exact);
        let ty = match (func, arg) {
            (AggFunc::Min | AggFunc::Max, Some(a)) => self.row(a, None)?.1,
            _ => ValueType::Number,
        };
        Ok(Scalar {
            sql: format!("\u{1}{i}\u{1}"),
            ty,
        })
    }

    /// A row expression over table aliases (`names`: columns renamed in a subquery).
    fn row(&mut self, b: &Bound, names: Option<&[(Id, String)]>) -> Result<(String, ValueType)> {
        Ok(match b {
            Bound::Number(n) => (number(*n), ValueType::Number),
            Bound::Text(s) => (string(s), ValueType::Text),
            Bound::Bool(v) => (v.to_string().to_uppercase(), ValueType::Bool),
            Bound::Blank => ("NULL".into(), ValueType::Number),
            Bound::Column { table, column } | Bound::Related { table, column } => {
                if let Some(n) = names.and_then(|ns| ns.iter().find(|(c, _)| c == column)) {
                    (n.1.clone(), ValueType::of(self.column_type(table, column)))
                } else {
                    self.column_sql(table, column)?
                }
            }
            Bound::Call { func, args } => {
                let args = args
                    .iter()
                    .map(|a| self.row(a, names))
                    .collect::<Result<Vec<_>>>()?;
                call_sql(func, &args)?
            }
            Bound::Switch {
                value,
                cases,
                default,
            } => {
                let v = self.row(value, names)?;
                let mut parts = Vec::new();
                let mut thens = Vec::new();
                for (w, t) in cases {
                    parts.push(eq_sql(&v, &self.row(w, names)?));
                    thens.push(self.row(t, names)?);
                }
                if let Some(d) = default {
                    thens.push(self.row(d, names)?);
                }
                let (t, ty) = unify(&thens);
                let mut sql = String::from("(CASE");
                for (p, x) in parts.iter().zip(&t) {
                    sql.push_str(&format!(" WHEN {p} THEN {x}"));
                }
                if default.is_some() {
                    sql.push_str(&format!(" ELSE {}", t.last().unwrap()));
                }
                sql.push_str(" END)");
                (sql, ty)
            }
            Bound::In { value, list } => {
                let v = self.row(value, names)?;
                let mut parts = Vec::new();
                for item in list {
                    parts.push(eq_sql(&v, &self.row(item, names)?));
                }
                (format!("({})", parts.join(" OR ")), ValueType::Bool)
            }
            Bound::Unary { op, expr } => unary_sql(*op, &self.row(expr, names)?),
            Bound::Binary { op, left, right } => {
                binary_sql(*op, &self.row(left, names)?, &self.row(right, names)?)
            }
            _ => {
                return Err(CompileError::Unsupported(
                    "this expression in a row context".into(),
                ));
            }
        })
    }

    /// Per table, the distinct visible tuples of the visual's grouping columns on it
    /// (with the blank member): (table, CTE name, key names).
    fn key_domains(&mut self) -> Result<Vec<(Id, String, Vec<String>)>> {
        if !self.dks.is_empty() || self.top_keys.is_empty() {
            return Ok(self.dks.clone());
        }
        let ctx = Ctx {
            keys: Vec::new(),
            filters: self.top_filters.clone(),
            use_rels: Vec::new(),
        };
        let mut tables: Vec<Id> = Vec::new();
        for k in &self.top_keys {
            if !tables.contains(&k.table) {
                tables.push(k.table.clone());
            }
        }
        for (i, t) in tables.iter().enumerate() {
            let (_, preds, from, _) = self.scope(t, &ctx, std::slice::from_ref(t), true)?;
            let keys: Vec<Key> = self
                .top_keys
                .iter()
                .filter(|k| &k.table == t)
                .cloned()
                .collect();
            let cols = keys
                .iter()
                .map(|k| {
                    Ok(format!(
                        "{} AS {}",
                        self.column_sql(&k.table, &k.column)?.0,
                        k.name
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            let sql = self.distinct_with_blank(&cols.join(", "), t, &from, &preds)?;
            let name = format!("dk{i}");
            self.ctes.push(CteDef {
                name: name.clone(),
                sql,
                keys: Vec::new(),
                exact: true,
            });
            self.dks
                .push((t.clone(), name, keys.into_iter().map(|k| k.name).collect()));
        }
        Ok(self.dks.clone())
    }

    /// The domain of the visual's keys: the key domains of every table, cross joined.
    fn domain(&mut self) -> Result<String> {
        if let Some(d) = &self.dom {
            return Ok(d.clone());
        }
        let dks = self.key_domains()?;
        let sql = if dks.is_empty() {
            "SELECT 1 AS one".to_string()
        } else {
            format!(
                "SELECT * FROM {}",
                dks.iter()
                    .map(|(_, n, _)| n.clone())
                    .collect::<Vec<_>>()
                    .join(" CROSS JOIN ")
            )
        };
        let name = "dom".to_string();
        self.ctes.push(CteDef {
            name: name.clone(),
            sql,
            keys: Vec::new(),
            exact: true,
        });
        self.dom = Some(name.clone());
        Ok(name)
    }

    /// The scalar SQL for a frame: `\u{1}n\u{1}` → `cN.v`, joining those CTEs.
    fn render(&self, s: &Scalar, frame_rel: &str) -> (String, String) {
        let mut sql = String::new();
        let mut used: BTreeSet<usize> = BTreeSet::new();
        let mut parts = s.sql.split('\u{1}');
        if let Some(first) = parts.next() {
            sql.push_str(first);
        }
        while let (Some(idx), Some(rest)) = (parts.next(), parts.next()) {
            let i: usize = idx.parse().unwrap_or(0);
            used.insert(i);
            sql.push_str(&format!("{}.v", self.ctes[i].name));
            sql.push_str(rest);
        }
        let mut from = format!("{frame_rel} AS f");
        for i in used {
            let c = &self.ctes[i];
            if c.keys.is_empty() {
                from.push_str(&format!(" CROSS JOIN {}", c.name));
            } else {
                let on: Vec<String> = c
                    .keys
                    .iter()
                    .map(|k| format!("f.{k} IS NOT DISTINCT FROM {}.{k}", c.name))
                    .collect();
                from.push_str(&format!(" LEFT JOIN {} ON {}", c.name, on.join(" AND ")));
            }
        }
        (sql, from)
    }

    /// Applies CALCULATE's changes to a context.
    fn calculate_ctx(
        &mut self,
        ctx: &Ctx,
        filters: &[FilterArg],
        frame: &Frame,
        rows: &[RowBinding],
    ) -> Result<Ctx> {
        // Filter arguments see the context before transition; the result starts after it.
        let outer = ctx.clone();
        let mut c = self.transition(ctx, rows);
        // New filters, evaluated in the outer context.
        let mut added: Vec<(CFilter, bool)> = Vec::new();
        // Time intelligence on a date table (marked, or joined on the date column)
        // replaces the filters on all of the table's columns, as in DAX.
        let mut date_tables: Vec<Id> = Vec::new();
        for fa in filters {
            let (inner, keep) = match fa {
                FilterArg::Keep(i) => (&**i, true),
                other => (other, false),
            };
            match inner {
                FilterArg::RemoveFilters { table, columns } => {
                    self.remove(&mut c, table, columns, None)
                }
                FilterArg::AllExcept { table, keep } => self.remove(&mut c, table, &[], Some(keep)),
                FilterArg::RemoveGrouping { table, columns } => {
                    c.keys.retain(|k| {
                        !(&k.table == table && (columns.is_empty() || columns.contains(&k.column)))
                    });
                }
                FilterArg::UseRelationship(r) => c.use_rels.push(r.clone()),
                FilterArg::RemoveAll => {
                    c.keys.clear();
                    c.filters.clear();
                }
                FilterArg::Table(t) => {
                    let before = added.len();
                    self.table_filter(t, keep, &outer, frame, rows, &mut added)?;
                    // A filter on a date table's date column replaces all its filters.
                    for (f, _) in &added[before..] {
                        if !keep && crate::replaces_date_table(self.model, &f.table, &f.columns) {
                            date_tables.push(f.table.clone());
                        }
                    }
                }
                FilterArg::Keep(_) => {
                    return Err(CompileError::Unsupported(
                        "KEEPFILTERS inside KEEPFILTERS".into(),
                    ));
                }
            }
        }
        c.keys.retain(|k| !date_tables.contains(&k.table));
        c.filters.retain(|f| !date_tables.contains(&f.table));
        // Filter arguments replace the filters already there, and are ANDed together.
        for (f, keep) in &added {
            if !keep && !f.columns.is_empty() {
                c.keys.retain(|k| !f.columns.contains(&k.column));
                c.filters
                    .retain(|x| !x.columns.iter().any(|col| f.columns.contains(col)));
            }
        }
        c.filters.extend(added.into_iter().map(|(f, _)| f));
        Ok(c)
    }

    /// The values of some columns visible in a context (VALUES), as a value set
    /// filter: one row per visible tuple and per key of the context the table reaches.
    fn value_set(&mut self, table_id: &str, columns: &[Id], outer: &Ctx) -> Result<CFilter> {
        let (keys, preds, from, _) = self.scope(table_id, outer, &[table_id.to_string()], true)?;
        let mut select = Vec::new();
        for (k, e) in &keys {
            select.push(format!("{e} AS {}", k.name));
        }
        let keys: Vec<Key> = keys.into_iter().map(|(k, _)| k).collect();
        for (i, c) in columns.iter().enumerate() {
            select.push(format!("{} AS v{i}", self.column_sql(table_id, c)?.0));
        }
        let sql = self.distinct_with_blank(&select.join(", "), table_id, &from, &preds)?;
        let set = self.add_cte(sql, Vec::new(), false);
        Ok(CFilter {
            table: table_id.to_string(),
            columns: columns.to_vec(),
            pred: Pred::InSet {
                set,
                columns: columns.to_vec(),
                keys,
            },
        })
    }

    /// The dates a time-intelligence function returns, as a value set: computed from
    /// the dates visible under each key of the outer context, and kept to the dates
    /// the column has.
    fn date_set(
        &mut self,
        table_id: &str,
        column: &str,
        op: &DateOp,
        outer: &Ctx,
        frame: &Frame,
        rows: &[RowBinding],
    ) -> Result<CFilter> {
        let (mut keys, preds, from, _) =
            self.scope(table_id, outer, &[table_id.to_string()], true)?;
        let col = format!("CAST({} AS DATE)", self.column_sql(table_id, column)?.0);
        // Ends given as expressions (`EOMONTH ( MAX ( … ), -1 )`, `TODAY ()`) are values
        // of the outer frame: then the first and last visible dates are too, and the set
        // is keyed by the frame's keys.
        let exprs: Vec<&Bound> = match op {
            DateOp::InPeriod(a, ..) => vec![a],
            DateOp::Between(a, b) => vec![a, b],
            _ => Vec::new(),
        }
        .into_iter()
        .filter_map(|a| match a {
            Anchor::Expr(e) => Some(&**e),
            _ => None,
        })
        .collect();
        let per_frame = if exprs.is_empty() {
            None
        } else {
            let date_of = |f: AggFunc| Bound::Aggregate {
                func: f,
                source: TableExpr::Table(table_id.to_string()),
                arg: Some(Box::new(Bound::Column {
                    table: table_id.to_string(),
                    column: column.to_string(),
                })),
            };
            let mut parts = vec![
                self.scalar(&date_of(AggFunc::Min), outer, frame, rows)?,
                self.scalar(&date_of(AggFunc::Max), outer, frame, rows)?,
            ];
            for e in &exprs {
                parts.push(self.scalar(e, outer, frame, rows)?);
            }
            let rel = if frame.rel == "k" {
                self.domain()?
            } else {
                frame.rel.clone()
            };
            let all = Scalar {
                sql: parts.iter().map(|p| p.sql.clone()).collect(),
                ty: ValueType::Date,
            };
            let (_, from) = self.render(&all, &rel);
            let mut select: Vec<String> = frame
                .keys
                .iter()
                .map(|k| format!("f.{0} AS {0}", k.name))
                .collect();
            for (i, p) in parts.iter().enumerate() {
                let (v, _) = self.render(p, &rel);
                let name = match i {
                    0 => "lo".to_string(),
                    1 => "hi".to_string(),
                    _ => format!("a{}", i - 2),
                };
                select.push(format!("{} AS {name}", date_sql(&(v, p.ty))));
            }
            keys = frame
                .keys
                .iter()
                .map(|k| (k.clone(), String::new()))
                .collect();
            Some(format!("(SELECT {} FROM {from})", select.join(", ")))
        };
        let names: Vec<String> = keys.iter().map(|(k, _)| k.name.clone()).collect();
        let mut select: Vec<String> = keys
            .iter()
            .map(|(k, e)| format!("{e} AS {}", k.name))
            .collect();
        select.push(format!("{col} AS v"));
        let mut wh = preds;
        wh.push(format!("{col} IS NOT NULL"));
        let visible = format!(
            "SELECT DISTINCT {} FROM {from} WHERE {}",
            select.join(", "),
            wh.join(" AND ")
        );
        let (_, _, all_from, _) = self.scope(table_id, &Ctx::default(), &[], false)?;
        let all = format!("SELECT DISTINCT {col} AS d FROM {all_from} WHERE {col} IS NOT NULL");
        let by = |p: &str| {
            names
                .iter()
                .map(|k| format!("{p}.{k}, "))
                .collect::<String>()
        };
        let iv = |n: i32, p: Period| match p {
            Period::Day => format!("to_days({n})"),
            Period::Month => format!("to_months({n})"),
            Period::Quarter => format!("to_months({})", 3 * n),
            Period::Year => format!("to_years({n})"),
        };
        let date = |e: String| format!("CAST({e} AS DATE)");
        let trunc = |p: Period, e: &str| date(format!("date_trunc('{}', {e})", p.sql()));
        // DATESINPERIOD and DATESBETWEEN dates: the first or last visible date, a DATE,
        // or `open` for no bound.
        let mut next_expr = 0;
        let mut anchor = |a: &Anchor, open: &str| -> Result<String> {
            Ok(match a {
                Anchor::First => "s.lo".into(),
                Anchor::Last => "s.hi".into(),
                Anchor::Date(y, m, d) => {
                    let n = |x: i32| (x.to_string(), ValueType::Number);
                    call_sql(&ScalarFunc::Date, &[n(*y), n(*m), n(*d)])?.0
                }
                Anchor::Open => open.into(),
                // A blank end of DATESBETWEEN is open, as BLANK () is.
                Anchor::Expr(_) => {
                    next_expr += 1;
                    format!("coalesce(s.a{}, {open})", next_expr - 1)
                }
            })
        };
        // The first and last visible date per key.
        let span = per_frame.unwrap_or_else(|| {
            format!(
                "(SELECT {}min(v) AS lo, max(v) AS hi FROM ({visible}) AS b{})",
                by("b"),
                if names.is_empty() {
                    String::new()
                } else {
                    format!(
                        " GROUP BY {}",
                        names
                            .iter()
                            .map(|k| format!("b.{k}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            )
        });
        let between = |lo: String, hi: String| {
            format!(
                "SELECT DISTINCT {}a.d AS v0 FROM {span} AS s JOIN ({all}) AS a ON a.d BETWEEN {lo} AND {hi}",
                by("s")
            )
        };
        let sql = match op.clone() {
            DateOp::Shift(n, p) => {
                let moved = date(format!("b.v + {}", iv(n, p)));
                let month_end = if p == Period::Day {
                    String::new()
                } else {
                    format!(
                        " OR (b.v = last_day(b.v) AND a.d > {moved} AND a.d <= last_day({moved}))"
                    )
                };
                format!(
                    "SELECT DISTINCT {}a.d AS v0 FROM ({visible}) AS b JOIN ({all}) AS a ON a.d = {moved}{month_end}",
                    by("b")
                )
            }
            DateOp::Parallel(n, p) => between(
                trunc(p, &format!("s.lo + {}", iv(n, p))),
                date(format!(
                    "{} + {} - to_days(1)",
                    trunc(p, &format!("s.hi + {}", iv(n, p))),
                    iv(1, p)
                )),
            ),
            DateOp::Previous(p) => between(
                date(format!("{} - {}", trunc(p, "s.lo"), iv(1, p))),
                date(format!("{} - to_days(1)", trunc(p, "s.lo"))),
            ),
            DateOp::Next(p) => between(
                date(format!("{} + {}", trunc(p, "s.hi"), iv(1, p))),
                date(format!("{} + {} - to_days(1)", trunc(p, "s.hi"), iv(2, p))),
            ),
            DateOp::ToDate(p, None) => between(trunc(p, "s.hi"), "s.hi".into()),
            DateOp::ToDate(_, Some((m, d))) => {
                let end = |y: &str| format!("(make_date({y}, {m}, 1) + to_days({}))", d - 1);
                between(
                    date(format!(
                        "CASE WHEN s.hi > {this} THEN {this} ELSE {last} END + to_days(1)",
                        this = end("year(s.hi)"),
                        last = end("year(s.hi) - 1")
                    )),
                    "s.hi".into(),
                )
            }
            DateOp::InPeriod(a, n, p) => {
                let at = anchor(&a, "s.lo")?;
                if n < 0 {
                    let moved = date(format!("{at} + {}", iv(n, p)));
                    // Back from a month's last day to the target month's last day.
                    let back = if p == Period::Day {
                        moved
                    } else {
                        format!(
                            "(CASE WHEN {at} = last_day({at}) THEN last_day({moved}) ELSE {moved} END)"
                        )
                    };
                    between(date(format!("{back} + to_days(1)")), at)
                } else {
                    between(
                        at.clone(),
                        date(format!("{at} + {} - to_days(1)", iv(n, p))),
                    )
                }
            }
            DateOp::Between(a, b) => between(
                anchor(&a, "DATE '0001-01-01'")?,
                anchor(&b, "DATE '9999-12-31'")?,
            ),
        };
        let set = self.add_cte(sql, Vec::new(), false);
        Ok(CFilter {
            table: table_id.to_string(),
            columns: vec![column.to_string()],
            pred: Pred::InSet {
                set,
                columns: vec![column.to_string()],
                keys: keys.into_iter().map(|(k, _)| k).collect(),
            },
        })
    }

    /// ALL / REMOVEFILTERS / ALLEXCEPT.
    fn remove(&self, c: &mut Ctx, table: &str, columns: &[Id], except: Option<&[Id]>) {
        let expanded: Vec<Id> = if columns.is_empty() {
            let reach = Reach::with(self.model, table, &c.use_rels);
            self.model
                .tables
                .iter()
                .filter(|t| reach.reaches(table, &t.id))
                .map(|t| t.id.clone())
                .collect()
        } else {
            Vec::new()
        };
        let (mut k, mut f) = c.touches(columns, &expanded);
        if let Some(keep) = except {
            k.retain(|&i| !keep.contains(&c.keys[i].column));
            f.retain(|&i| !c.filters[i].columns.iter().all(|x| keep.contains(x)));
        }
        c.remove(&k, &f);
    }

    /// A table used as a CALCULATE filter.
    fn table_filter(
        &mut self,
        t: &TableExpr,
        keep: bool,
        outer: &Ctx,
        frame: &Frame,
        rows: &[RowBinding],
        added: &mut Vec<(CFilter, bool)>,
    ) -> Result<()> {
        let local = |c: &Bound, src: &TableExpr| src.table().is_some_and(|tb| row_local(c, tb));
        match t {
            TableExpr::Filter { source, condition }
                if local(condition, source)
                    && matches!(
                        &**source,
                        TableExpr::All { .. } | TableExpr::Table(_) | TableExpr::Values { .. }
                    ) =>
            {
                match &**source {
                    // FILTER ( ALL ( columns ), condition ): the condition replaces those columns' filters.
                    TableExpr::All { table, columns } => added.push((
                        CFilter {
                            table: table.clone(),
                            columns: columns.clone(),
                            pred: Pred::Cond((**condition).clone()),
                        },
                        keep,
                    )),
                    // FILTER ( Table or VALUES, condition ): visible rows that satisfy it.
                    TableExpr::Table(table) | TableExpr::Values { table, .. } => added.push((
                        CFilter {
                            table: table.clone(),
                            columns: Vec::new(),
                            pred: Pred::Cond((**condition).clone()),
                        },
                        true,
                    )),
                    _ => unreachable!("matched above"),
                }
            }
            // TOPN as a filter: its rows per frame row.
            TableExpr::TopN { source, .. } => {
                let replaces = matches!(
                    &**source,
                    TableExpr::All { .. } | TableExpr::AllSelected { .. }
                );
                let f = self.filter_set(t, outer, frame, rows)?;
                added.push((f, keep || !replaces));
            }
            // Conditions that need the frame (measures, outer values, outer rows), and
            // FILTERs over other tables: the passing rows per frame row.
            TableExpr::Filter { source, .. } => {
                let mut base = &**source;
                while let TableExpr::Filter { source, .. } = base {
                    base = source;
                }
                // Over ALL or ALLSELECTED the rows replace the filters on their columns;
                // over visible rows they can only narrow them.
                let replaces =
                    matches!(base, TableExpr::All { .. } | TableExpr::AllSelected { .. });
                let f = self.filter_set(t, outer, frame, rows)?;
                added.push((f, keep || !replaces));
            }
            TableExpr::Values { table, columns } => {
                added.push((self.value_set(table, columns, outer)?, keep))
            }
            TableExpr::AllSelected { .. } => {
                let (t, c) = unselect(t, outer).expect("ALLSELECTED");
                self.table_filter(&t, keep, &c, frame, rows, added)?
            }
            TableExpr::Table(table) => {
                // VALUES ( Table ): its rows, by the key column when it has one.
                let tab = self
                    .model
                    .table(table)
                    .ok_or_else(|| CompileError::Field("missing table".into()))?;
                let cols: Vec<Id> = match tab.columns.iter().find(|c| c.is_key) {
                    Some(k) => vec![k.id.clone()],
                    None => tab.columns.iter().map(|c| c.id.clone()).collect(),
                };
                added.push((self.value_set(table, &cols, outer)?, keep));
            }
            TableExpr::All { table, columns } => added.push((
                CFilter {
                    table: table.clone(),
                    columns: if columns.is_empty() {
                        self.model
                            .table(table)
                            .map(|t| t.columns.iter().map(|c| c.id.clone()).collect())
                            .unwrap_or_default()
                    } else {
                        columns.clone()
                    },
                    pred: Pred::Cond(Bound::Bool(true)),
                },
                keep,
            )),
            TableExpr::List(_) => {
                return Err(CompileError::Unsupported(
                    "a list of values as a filter (use TREATAS)".into(),
                ));
            }
            TableExpr::Dates { table, column, op } => {
                added.push((self.date_set(table, column, op, outer, frame, rows)?, keep))
            }
        }
        Ok(())
    }

    /// Context transition: the current rows become keys, replacing the keys and filters
    /// on every column they have.
    fn transition(&self, ctx: &Ctx, rows: &[RowBinding]) -> Ctx {
        let mut c = ctx.clone();
        for r in rows {
            let all = self
                .model
                .table(&r.table)
                .is_some_and(|t| t.columns.iter().all(|col| r.covers.contains(&col.id)));
            c.keys.retain(|k| !r.covers.contains(&k.column));
            c.filters.retain(|f| {
                if f.columns.is_empty() {
                    // A condition on the whole table (FILTER ( Table, … )).
                    !(all && f.table == r.table)
                } else {
                    !f.columns.iter().any(|col| r.covers.contains(col))
                }
            });
            for col in &r.identity {
                if let Some((_, name)) = r.columns.iter().find(|(c, _)| c == col) {
                    c.keys.push(Key {
                        name: name.clone(),
                        table: r.table.clone(),
                        column: col.clone(),
                    });
                }
            }
        }
        c
    }

    fn measure(
        &mut self,
        id: &str,
        ctx: &Ctx,
        frame: &Frame,
        rows: &[RowBinding],
    ) -> Result<Scalar> {
        let m = self
            .model
            .tables
            .iter()
            .flat_map(|t| &t.measures)
            .find(|m| m.id == id)
            .ok_or_else(|| {
                CompileError::Field(format!("a field refers to a missing measure ({id})"))
            })?;
        if self.stack.iter().any(|s| s == id) {
            return Err(CompileError::Circular(m.name.clone()));
        }
        let (bound, diags) = charted_dax::check(self.model, &m.expression);
        let bound = bound.ok_or_else(|| CompileError::Measure {
            measure: m.name.clone(),
            message: diags
                .first()
                .map(|d| d.message.clone())
                .unwrap_or_else(|| "it can't be evaluated".into()),
        })?;
        let inner = self.transition(ctx, rows);
        self.stack.push(id.to_string());
        let saved = std::mem::take(&mut self.vars);
        let s = self.scalar(&bound, &inner, frame, &[]);
        self.vars = saved;
        self.stack.pop();
        s
    }

    /// An iterator whose expression needs context transition: a frame per iterated row.
    fn iterate(
        &mut self,
        func: AggFunc,
        source: &TableExpr,
        arg: &Bound,
        ctx: &Ctx,
        frame: &Frame,
        rows: &[RowBinding],
    ) -> Result<Scalar> {
        let rf = self.row_frame(source, &[arg], ctx, frame, rows)?;
        let frame_name = rf.frame.rel.clone();
        let filter_sql = rf.conds;
        let value = self.scalar(arg, ctx, &rf.frame, &rf.rows)?;
        let ty = value.ty;
        // Join every CTE the value and the conditions use, then render each.
        let combined = Scalar {
            sql: std::iter::once(value.sql.clone())
                .chain(filter_sql.iter().cloned())
                .collect(),
            ty,
        };
        let (_, from) = self.render(&combined, &frame_name);
        let (val_sql, _) = self.render(&value, &frame_name);
        let cond_sql: Vec<String> = filter_sql
            .iter()
            .map(|f| {
                self.render(
                    &Scalar {
                        sql: f.clone(),
                        ty: ValueType::Bool,
                    },
                    &frame_name,
                )
                .0
            })
            .collect();
        let (agg, _) = agg_sql(func, Some((val_sql, ty)));
        let keys: Vec<String> = frame.keys.iter().map(|k| k.name.clone()).collect();
        let mut select: Vec<String> = keys.iter().map(|k| format!("f.{k} AS {k}")).collect();
        select.push(format!("{agg} AS v"));
        let mut out = format!("SELECT {} FROM {from}", select.join(", "));
        if !cond_sql.is_empty() {
            out.push_str(&format!(" WHERE {}", cond_sql.join(" AND ")));
        }
        if !keys.is_empty() {
            out.push_str(&format!(
                " GROUP BY {}",
                keys.iter()
                    .map(|k| format!("f.{k}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        // Built over the key domain, so its keys are combinations the visual can show.
        let i = self.add_cte(out, keys, true);
        Ok(Scalar {
            sql: format!("\u{1}{i}\u{1}"),
            ty: if matches!(func, AggFunc::Min | AggFunc::Max) {
                ty
            } else {
                ValueType::Number
            },
        })
    }

    /// TOPN's rows: the source's frame rows ranked by the order values within each outer
    /// frame row (blank as zero), the first `n` kept with their ties.
    #[allow(clippy::too_many_arguments)]
    fn top_rows(
        &mut self,
        n: &Bound,
        source: &TableExpr,
        order: &[(Bound, bool)],
        conds: Vec<&Bound>,
        ctx: &Ctx,
        frame: &Frame,
        rows: &[RowBinding],
    ) -> Result<RowFrame> {
        let exprs: Vec<&Bound> = order.iter().map(|(o, _)| o).collect();
        let rf = self.row_frame(source, &exprs, ctx, frame, rows)?;
        let binding = rf.rows.last().cloned().expect("the iterated row");
        let mut parts = Vec::new();
        for (o, _) in order {
            parts.push(self.scalar(o, ctx, &rf.frame, &rf.rows)?);
        }
        let count = self.scalar(n, ctx, frame, rows)?;
        let rel = rf.frame.rel.clone();
        let combined = Scalar {
            sql: parts
                .iter()
                .map(|p| p.sql.clone())
                .chain([count.sql.clone()])
                .chain(rf.conds.iter().cloned())
                .collect(),
            ty: ValueType::Number,
        };
        let (_, from) = self.render(&combined, &rel);
        let mut by = Vec::new();
        for (p, (_, asc)) in parts.iter().zip(order) {
            let (x, _) = self.render(p, &rel);
            let x = blank_as(&(x, p.ty), p.ty);
            let x = if p.ty == ValueType::Text {
                format!("lower({x})")
            } else {
                x
            };
            by.push(format!("{x} {}", if *asc { "ASC" } else { "DESC" }));
        }
        let (n_sql, _) = self.render(&count, &rel);
        let inner_conds: Vec<String> = rf
            .conds
            .iter()
            .map(|c| {
                self.render(
                    &Scalar {
                        sql: c.clone(),
                        ty: ValueType::Bool,
                    },
                    &rel,
                )
                .0
            })
            .collect();
        let keys: Vec<String> = frame.keys.iter().map(|k| k.name.clone()).collect();
        let partition = if keys.is_empty() {
            String::new()
        } else {
            format!(
                "PARTITION BY {} ",
                keys.iter()
                    .map(|k| format!("f.{k}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let order_by = if by.is_empty() {
            "1".to_string()
        } else {
            by.join(", ")
        };
        let wh = if inner_conds.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", inner_conds.join(" AND "))
        };
        let row_cols: Vec<String> = binding
            .columns
            .iter()
            .map(|(_, c)| c.clone())
            .chain(binding.related.iter().map(|(_, c)| c.clone()))
            .collect();
        let select: Vec<String> = keys
            .iter()
            .chain(row_cols.iter())
            .map(|c| format!("f.{c} AS {c}"))
            .collect();
        let rows_name = self.fresh("rows");
        let sql = format!(
            "SELECT {} FROM (SELECT f.*, rank() OVER ({partition}ORDER BY {order_by}) AS rk, {n_sql} AS n FROM {from}{wh}) AS f WHERE f.rk <= f.n",
            if select.is_empty() {
                "1 AS one".to_string()
            } else {
                select.join(", ")
            }
        );
        self.ctes.push(CteDef {
            name: rows_name.clone(),
            sql,
            keys: Vec::new(),
            exact: true,
        });
        self.join_rows(&rows_name, &keys, binding, conds, ctx, frame, rows)
    }

    /// CONCATENATEX: per outer frame row, the iterated rows' text joined in order.
    #[allow(clippy::too_many_arguments)]
    fn concat_x(
        &mut self,
        source: &TableExpr,
        expr: &Bound,
        delimiter: &str,
        order: Option<(&Bound, bool)>,
        ctx: &Ctx,
        frame: &Frame,
        rows: &[RowBinding],
    ) -> Result<Scalar> {
        let mut exprs = vec![expr];
        if let Some((o, _)) = order {
            exprs.push(o);
        }
        let rf = self.row_frame(source, &exprs, ctx, frame, rows)?;
        let value = self.scalar(expr, ctx, &rf.frame, &rf.rows)?;
        let ord = match order {
            Some((o, asc)) => Some((self.scalar(o, ctx, &rf.frame, &rf.rows)?, asc)),
            None => None,
        };
        let rel = rf.frame.rel.clone();
        let combined = Scalar {
            sql: [value.sql.clone()]
                .into_iter()
                .chain(ord.iter().map(|(o, _)| o.sql.clone()))
                .chain(rf.conds.iter().cloned())
                .collect(),
            ty: ValueType::Text,
        };
        let (_, from) = self.render(&combined, &rel);
        let (v, _) = self.render(&value, &rel);
        let t = text_sql(&(v, value.ty));
        let mut by = Vec::new();
        if let Some((o, asc)) = &ord {
            let (x, _) = self.render(o, &rel);
            let x = blank_as(&(x, o.ty), o.ty);
            let x = if o.ty == ValueType::Text {
                format!("lower({x})")
            } else {
                x
            };
            by.push(format!("{x} {}", if *asc { "ASC" } else { "DESC" }));
        }
        by.push(format!("lower({t})"));
        by.push(t.clone());
        let conds: Vec<String> = rf
            .conds
            .iter()
            .map(|c| {
                self.render(
                    &Scalar {
                        sql: c.clone(),
                        ty: ValueType::Bool,
                    },
                    &rel,
                )
                .0
            })
            .collect();
        let keys: Vec<String> = frame.keys.iter().map(|k| k.name.clone()).collect();
        let mut select: Vec<String> = keys.iter().map(|k| format!("f.{k} AS {k}")).collect();
        select.push(format!(
            "string_agg({t}, {} ORDER BY {}) AS v",
            string(delimiter),
            by.join(", ")
        ));
        let mut sql = format!("SELECT {} FROM {from}", select.join(", "));
        if !conds.is_empty() {
            sql.push_str(&format!(" WHERE {}", conds.join(" AND ")));
        }
        if !keys.is_empty() {
            sql.push_str(&format!(
                " GROUP BY {}",
                keys.iter()
                    .map(|k| format!("f.{k}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let i = self.add_cte(sql, keys, true);
        Ok(Scalar {
            sql: format!("\u{1}{i}\u{1}"),
            ty: ValueType::Text,
        })
    }

    /// RANKX: per outer frame row, 1 + the iterated rows whose value beats the outer
    /// value (distinct values when dense). Blank compares as zero or empty text.
    #[allow(clippy::too_many_arguments)]
    fn rank(
        &mut self,
        source: &TableExpr,
        expr: &Bound,
        value: Option<&Bound>,
        ascending: bool,
        dense: bool,
        ctx: &Ctx,
        frame: &Frame,
        rows: &[RowBinding],
    ) -> Result<Scalar> {
        let rf = self.row_frame(source, &[expr], ctx, frame, rows)?;
        let inner = self.scalar(expr, ctx, &rf.frame, &rf.rows)?;
        let outer = self.scalar(value.unwrap_or(expr), ctx, frame, rows)?;
        let rel = rf.frame.rel.clone();
        let combined = Scalar {
            sql: [inner.sql.clone(), outer.sql.clone()]
                .into_iter()
                .chain(rf.conds.iter().cloned())
                .collect(),
            ty: inner.ty,
        };
        let (_, from) = self.render(&combined, &rel);
        let (iv, _) = self.render(&inner, &rel);
        let (ov, _) = self.render(&outer, &rel);
        let like = if inner.ty == ValueType::Text || outer.ty == ValueType::Text {
            ValueType::Text
        } else {
            inner.ty
        };
        let norm = |x: String, ty: ValueType| {
            let b = blank_as(&(x, ty), like);
            if like == ValueType::Text {
                format!("lower({b})")
            } else {
                b
            }
        };
        // Rows exist where some value has data, as for the other aggregates (reading the
        // iterated rows' own columns isn't data).
        let having = if row_pure(expr) {
            format!(" HAVING bool_or({ov} IS NOT NULL)")
        } else {
            format!(" HAVING count({iv}) > 0 OR bool_or({ov} IS NOT NULL)")
        };
        let (iv, ov) = (norm(iv, inner.ty), norm(ov, outer.ty));
        let beats = format!("{iv} {} {ov}", if ascending { "<" } else { ">" });
        let count = if dense {
            format!("count(DISTINCT {iv}) FILTER (WHERE {beats})")
        } else {
            format!("count(*) FILTER (WHERE {beats})")
        };
        let conds: Vec<String> = rf
            .conds
            .iter()
            .map(|c| {
                self.render(
                    &Scalar {
                        sql: c.clone(),
                        ty: ValueType::Bool,
                    },
                    &rel,
                )
                .0
            })
            .collect();
        let keys: Vec<String> = frame.keys.iter().map(|k| k.name.clone()).collect();
        let mut select: Vec<String> = keys.iter().map(|k| format!("f.{k} AS {k}")).collect();
        select.push(format!("{count} AS v"));
        let mut sql = format!("SELECT {} FROM {from}", select.join(", "));
        if !conds.is_empty() {
            sql.push_str(&format!(" WHERE {}", conds.join(" AND ")));
        }
        if !keys.is_empty() {
            sql.push_str(&format!(
                " GROUP BY {}",
                keys.iter()
                    .map(|k| format!("f.{k}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        // A card always has its one row.
        if !keys.is_empty() {
            sql.push_str(&having);
        }
        let i = self.add_cte(sql, keys, true);
        Ok(Scalar {
            sql: format!("(1 + coalesce(\u{1}{i}\u{1}, 0))"),
            ty: ValueType::Number,
        })
    }

    /// A FILTER whose condition needs the outer frame (its measures, or values such as
    /// `MAX ( 'Date'[Date] )` from the outer context), used as a CALCULATE filter: the
    /// rows that pass, per row of the frame, as a value set keyed by the frame's keys.
    fn filter_set(
        &mut self,
        source: &TableExpr,
        ctx: &Ctx,
        frame: &Frame,
        rows: &[RowBinding],
    ) -> Result<CFilter> {
        let rf = self.row_frame(source, &[], ctx, frame, rows)?;
        let b = rf.rows.last().expect("the iterated row");
        let ident: Vec<(Id, String)> = b
            .columns
            .iter()
            .filter(|(c, _)| b.identity.contains(c))
            .cloned()
            .collect();
        let table = b.table.clone();
        let covers = b.covers.clone();
        let cond = Scalar {
            sql: if rf.conds.is_empty() {
                "TRUE".into()
            } else {
                rf.conds
                    .iter()
                    .map(|c| format!("({c})"))
                    .collect::<Vec<_>>()
                    .join(" AND ")
            },
            ty: ValueType::Bool,
        };
        let (cond_sql, from) = self.render(&cond, &rf.frame.rel);
        let mut select: Vec<String> = frame
            .keys
            .iter()
            .map(|k| format!("f.{0} AS {0}", k.name))
            .collect();
        for (i, (_, n)) in ident.iter().enumerate() {
            select.push(format!("f.{n} AS v{i}"));
        }
        let sql = format!(
            "SELECT DISTINCT {} FROM {from} WHERE {cond_sql}",
            select.join(", ")
        );
        let set = self.add_cte(sql, Vec::new(), false);
        // Matched on the columns that identify a row; replaces filters on all it has.
        let columns: Vec<Id> = ident.into_iter().map(|(c, _)| c).collect();
        Ok(CFilter {
            table,
            columns: covers,
            pred: Pred::InSet {
                set,
                columns,
                keys: frame.keys.clone(),
            },
        })
    }

    /// The rows of a table expression joined to the frame: a frame with a row per outer
    /// row and iterated row, the bindings that read them, and the FILTER conditions
    /// that need the frame (unrendered SQL; row-level ones are already applied).
    fn row_frame(
        &mut self,
        source: &TableExpr,
        exprs: &[&Bound],
        ctx: &Ctx,
        frame: &Frame,
        rows: &[RowBinding],
    ) -> Result<RowFrame> {
        let mut conds: Vec<&Bound> = Vec::new();
        let mut cur = source;
        while let TableExpr::Filter { source, condition } = cur {
            conds.push(condition);
            cur = source;
        }
        // The rows are read in their own context (ALLSELECTED drops the keys); the
        // expression is evaluated in the outer one.
        let selected = unselect(cur, ctx);
        let (cur, rows_ctx) = match &selected {
            Some((t, c)) => (t, c),
            None => (cur, ctx),
        };
        // TOPN: its source's rows per row of the frame, ranked within each.
        if let TableExpr::TopN { n, source, order } = cur {
            return self.top_rows(n, source, order, conds, ctx, frame, rows);
        }
        // A time-intelligence table: the dates of its set, per row of the frame.
        if let TableExpr::Dates { table, column, op } = cur {
            let f = self.date_set(table, column, op, rows_ctx, frame, rows)?;
            let Pred::InSet { set, keys, .. } = &f.pred else {
                unreachable!("date sets are value sets")
            };
            let reach: Vec<String> = keys
                .iter()
                .filter(|k| frame.keys.iter().any(|x| x.name == k.name))
                .map(|k| k.name.clone())
                .collect();
            let n = self.fresh("r");
            let mut select = reach.clone();
            select.push(format!("v0 AS {n}"));
            let rows_name = self.fresh("rows");
            let sql = format!(
                "SELECT DISTINCT {} FROM {}",
                select.join(", "),
                self.ctes[*set].name
            );
            self.ctes.push(CteDef {
                name: rows_name.clone(),
                sql,
                keys: Vec::new(),
                exact: true,
            });
            let binding = RowBinding {
                table: table.clone(),
                columns: vec![(column.clone(), n)],
                identity: vec![column.clone()],
                covers: vec![column.clone()],
                related: Vec::new(),
            };
            return self.join_rows(&rows_name, &reach, binding, conds, ctx, frame, rows);
        }
        let (t, cols, apply_ctx) = match cur {
            TableExpr::Table(t) => (t.clone(), None, true),
            TableExpr::Values { table, columns } => (table.clone(), Some(columns.clone()), true),
            TableExpr::All { table, columns } => (
                table.clone(),
                if columns.is_empty() {
                    None
                } else {
                    Some(columns.clone())
                },
                false,
            ),
            TableExpr::List(_) => {
                return Err(CompileError::Unsupported(
                    "iterating a list of values".into(),
                ));
            }
            TableExpr::Filter { .. }
            | TableExpr::AllSelected { .. }
            | TableExpr::Dates { .. }
            | TableExpr::TopN { .. } => {
                unreachable!("filters, ALLSELECTED, dates and TOPN were handled")
            }
        };
        let tab = table(self.model, &t)?;
        // A whole row has every column, identified by the table's key column when it
        // has one; VALUES and ALL of columns have just those.
        let (cols, identity): (Vec<Id>, Vec<Id>) = match cols {
            Some(c) => (c.clone(), c),
            None => {
                let all: Vec<Id> = tab.columns.iter().map(|c| c.id.clone()).collect();
                match tab.columns.iter().find(|c| c.is_key) {
                    Some(k) => (all, vec![k.id.clone()]),
                    None => (all.clone(), all),
                }
            }
        };
        let mut related = Vec::new();
        for a in exprs {
            related_refs(a, &mut related);
        }
        for c in &conds {
            related_refs(c, &mut related);
        }
        let extra: Vec<Id> = related.iter().map(|(tb, _)| tb.clone()).collect();
        let (keys, mut preds, from, _) = self.scope(&t, rows_ctx, &extra, apply_ctx)?;
        // Keys of the outer frame this table reaches join the rows to the frame.
        let reach_keys: Vec<(Key, String)> = keys
            .iter()
            .filter(|(k, _)| frame.keys.iter().any(|f| f.name == k.name))
            .cloned()
            .collect();
        let mut binding = RowBinding {
            table: t.clone(),
            columns: Vec::new(),
            identity,
            covers: cols.clone(),
            related: Vec::new(),
        };
        let mut select: Vec<String> = Vec::new();
        for (k, e) in &reach_keys {
            select.push(format!("{e} AS {}", k.name));
        }
        for c in &cols {
            let n = self.fresh("r");
            select.push(format!("{} AS {n}", self.column_sql(&t, c)?.0));
            binding.columns.push((c.clone(), n));
        }
        for (rt, rc) in &related {
            let n = self.fresh("r");
            select.push(format!("{} AS {n}", self.column_sql(rt, rc)?.0));
            binding.related.push(((rt.clone(), rc.clone()), n));
        }
        // Row-level FILTER conditions apply here; others after the frame is built.
        let mut later: Vec<&Bound> = Vec::new();
        for c in conds {
            if row_local(c, &t) {
                preds.push(bool_of(&self.row(c, None)?));
            } else {
                later.push(c);
            }
        }
        let rows_name = self.fresh("rows");
        let rows_sql = if matches!(cur, TableExpr::Table(_)) {
            let wh = if preds.is_empty() {
                String::new()
            } else {
                format!(" WHERE {}", preds.join(" AND "))
            };
            // The rowid keeps duplicate rows apart; DISTINCT undoes repeats from joins.
            select.push(format!(
                "{}.rowid AS {}",
                alias(self.model, &t),
                self.fresh("rid")
            ));
            format!("SELECT DISTINCT {} FROM {from}{wh}", select.join(", "))
        } else {
            // VALUES and ALL include the blank member.
            self.distinct_with_blank(&select.join(", "), &t, &from, &preds)?
        };
        self.ctes.push(CteDef {
            name: rows_name.clone(),
            sql: rows_sql,
            keys: Vec::new(),
            exact: true,
        });
        let reach: Vec<String> = reach_keys.into_iter().map(|(k, _)| k.name).collect();
        self.join_rows(&rows_name, &reach, binding, later, ctx, frame, rows)
    }

    /// The frame of an iterated rows CTE: each outer frame row joined to its rows (on
    /// the frame keys in `reach`), with the conditions that need the frame.
    #[allow(clippy::too_many_arguments)]
    fn join_rows(
        &mut self,
        rows_name: &str,
        reach: &[String],
        binding: RowBinding,
        later: Vec<&Bound>,
        ctx: &Ctx,
        frame: &Frame,
        rows: &[RowBinding],
    ) -> Result<RowFrame> {
        let t = binding.table.clone();
        // The new frame: outer frame rows joined to the iterated rows.
        let row_cols: Vec<String> = binding
            .columns
            .iter()
            .map(|(_, n)| n.clone())
            .chain(binding.related.iter().map(|(_, n)| n.clone()))
            .collect();
        let join = if reach.is_empty() {
            format!("CROSS JOIN {rows_name} AS r")
        } else {
            format!(
                "JOIN {rows_name} AS r ON {}",
                reach
                    .iter()
                    .map(|k| format!("o.{k} IS NOT DISTINCT FROM r.{k}"))
                    .collect::<Vec<_>>()
                    .join(" AND ")
            )
        };
        // At the top level the frame is the key domain (`k` itself depends on this).
        let outer_rel = if frame.rel == "k" {
            self.domain()?
        } else {
            frame.rel.clone()
        };
        let frame_name = self.fresh("frame");
        // The outer frame's columns (keys and outer rows' values) carry over.
        let mut fsel = vec!["o.*".to_string()];
        fsel.extend(row_cols.iter().map(|c| format!("r.{c}")));
        let fsel = fsel.join(", ");
        self.ctes.push(CteDef {
            name: frame_name.clone(),
            sql: format!("SELECT {fsel} FROM {outer_rel} AS o {join}"),
            keys: Vec::new(),
            exact: true,
        });
        let mut inner_keys = frame.keys.clone();
        for (c, n) in binding
            .columns
            .iter()
            .filter(|(c, _)| binding.identity.contains(c))
        {
            inner_keys.push(Key {
                name: n.clone(),
                table: t.clone(),
                column: c.clone(),
            });
        }
        let inner_frame = Frame {
            rel: frame_name.clone(),
            keys: inner_keys,
        };
        let mut inner_rows = rows.to_vec();
        inner_rows.push(binding);
        // Conditions with measures filter the frame rows.
        let mut conds_sql = Vec::new();
        for c in later {
            let s = self.scalar(c, ctx, &inner_frame, &inner_rows)?;
            conds_sql.push(bool_of(&(s.sql, s.ty)));
        }
        Ok(RowFrame {
            frame: inner_frame,
            rows: inner_rows,
            conds: conds_sql,
        })
    }

    /// Lowers a value in a filter context, over a frame.
    fn scalar(
        &mut self,
        b: &Bound,
        ctx: &Ctx,
        frame: &Frame,
        rows: &[RowBinding],
    ) -> Result<Scalar> {
        let lit = |sql: String, ty| Ok(Scalar { sql, ty });
        match b {
            Bound::Number(n) => lit(number(*n), ValueType::Number),
            Bound::Text(s) => lit(string(s), ValueType::Text),
            Bound::Bool(v) => lit(v.to_string().to_uppercase(), ValueType::Bool),
            Bound::Blank => lit("NULL".into(), ValueType::Number),
            Bound::Column { table, column } => {
                let name = rows
                    .iter()
                    .rev()
                    .find(|r| &r.table == table)
                    .and_then(|r| r.columns.iter().find(|(c, _)| c == column))
                    .map(|(_, n)| n.clone())
                    .ok_or_else(|| {
                        CompileError::Unsupported(
                            "this column reference outside the iterated columns".into(),
                        )
                    })?;
                lit(
                    format!("f.{name}"),
                    ValueType::of(self.column_type(table, column)),
                )
            }
            Bound::Related { table, column } => {
                let r = rows
                    .last()
                    .ok_or_else(|| CompileError::Field("RELATED needs a row context".into()))?;
                let name = if &r.table == table {
                    r.columns
                        .iter()
                        .find(|(c, _)| c == column)
                        .map(|(_, n)| n.clone())
                } else {
                    r.related
                        .iter()
                        .find(|((t, c), _)| t == table && c == column)
                        .map(|(_, n)| n.clone())
                };
                let name =
                    name.ok_or_else(|| CompileError::Unsupported("this RELATED lookup".into()))?;
                lit(
                    format!("f.{name}"),
                    ValueType::of(self.column_type(table, column)),
                )
            }
            Bound::Measure { measure, .. } => self.measure(measure, ctx, frame, rows),
            Bound::Var(name) => self
                .vars
                .iter()
                .rev()
                .find(|(n, _)| n == name)
                .map(|(_, s)| s.clone())
                .ok_or_else(|| CompileError::Field(format!("unknown variable {name}"))),
            Bound::Let { name, value, body } => {
                let v = self.scalar(value, ctx, frame, rows)?;
                self.vars.push((name.clone(), v));
                let out = self.scalar(body, ctx, frame, rows);
                self.vars.pop();
                out
            }
            Bound::ConcatX {
                source,
                expr,
                delimiter,
                order,
            } => self.concat_x(
                source,
                expr,
                delimiter,
                order.as_ref().map(|(o, asc)| (&**o, *asc)),
                ctx,
                frame,
                rows,
            ),
            Bound::Rank {
                source,
                expr,
                value,
                ascending,
                dense,
            } => self.rank(
                source,
                expr,
                value.as_deref(),
                *ascending,
                *dense,
                ctx,
                frame,
                rows,
            ),
            Bound::Aggregate { func, source, arg } => {
                let base = source.table().cloned().unwrap_or_default();
                let local = |b: &Bound| row_local(b, &base);
                let pure = arg.as_deref().is_none_or(local) && {
                    let mut ok = true;
                    let mut t = source;
                    while let TableExpr::Filter { source, condition } = t {
                        ok &= local(condition);
                        t = source;
                    }
                    // Date sets are per frame row: they always take the frame path.
                    ok && !matches!(t, TableExpr::Dates { .. } | TableExpr::TopN { .. })
                };
                if pure {
                    self.simple_aggregate(*func, source, arg.as_deref(), ctx)
                } else {
                    match arg.as_deref() {
                        Some(a) => self.iterate(*func, source, a, ctx, frame, rows),
                        // COUNTROWS: count the rows that pass.
                        None => self.iterate(
                            AggFunc::Count,
                            source,
                            &Bound::Number(1.0),
                            ctx,
                            frame,
                            rows,
                        ),
                    }
                }
            }
            Bound::Calculate { expr, filters } => {
                let c = self.calculate_ctx(ctx, filters, frame, rows)?;
                self.scalar(expr, &c, frame, rows)
            }
            Bound::Call { func, args } => {
                let a = args
                    .iter()
                    .map(|x| self.scalar(x, ctx, frame, rows).map(|s| (s.sql, s.ty)))
                    .collect::<Result<Vec<_>>>()?;
                let (sql, ty) = call_sql(func, &a)?;
                lit(sql, ty)
            }
            Bound::Info {
                func,
                table,
                column,
                alternate,
            } => match func {
                InfoFunc::IsInScope => lit(
                    (column.as_ref().is_some_and(|c| {
                        ctx.keys
                            .iter()
                            .any(|k| &k.column == c && self.top_keys.contains(k))
                    }))
                    .to_string()
                    .to_uppercase(),
                    ValueType::Bool,
                ),
                InfoFunc::IsFiltered => {
                    let hit = ctx.keys.iter().any(|k| {
                        &k.table == table && column.as_ref().is_none_or(|c| &k.column == c)
                    }) || ctx.filters.iter().any(|f| {
                        &f.table == table && column.as_ref().is_none_or(|c| f.columns.contains(c))
                    });
                    lit(hit.to_string().to_uppercase(), ValueType::Bool)
                }
                InfoFunc::SelectedValue | InfoFunc::HasOneValue => {
                    let col = column.clone().unwrap_or_default();
                    let ty = ValueType::of(self.column_type(table, &col));
                    if let Some(k) = ctx.keys.iter().find(|k| k.column == col) {
                        return if *func == InfoFunc::HasOneValue {
                            lit("TRUE".into(), ValueType::Bool)
                        } else {
                            lit(format!("f.{}", k.name), ty)
                        };
                    }
                    let col_b = Bound::Column {
                        table: table.clone(),
                        column: col.clone(),
                    };
                    let n = self.simple_aggregate(
                        AggFunc::DistinctCount,
                        &TableExpr::Values {
                            table: table.clone(),
                            columns: vec![col.clone()],
                        },
                        Some(&col_b),
                        ctx,
                    )?;
                    if *func == InfoFunc::HasOneValue {
                        return lit(format!("coalesce({} = 1, FALSE)", n.sql), ValueType::Bool);
                    }
                    let v = self.simple_aggregate(
                        AggFunc::Min,
                        &TableExpr::Table(table.clone()),
                        Some(&col_b),
                        ctx,
                    )?;
                    let alt = match alternate {
                        Some(a) => self.scalar(a, ctx, frame, rows)?,
                        None => Scalar {
                            sql: "NULL".into(),
                            ty,
                        },
                    };
                    let (parts, ty) = unify(&[(v.sql, ty), (alt.sql, alt.ty)]);
                    lit(
                        format!(
                            "(CASE WHEN {} = 1 THEN {} ELSE {} END)",
                            n.sql, parts[0], parts[1]
                        ),
                        ty,
                    )
                }
            },
            Bound::Switch {
                value,
                cases,
                default,
            } => {
                let v = self.scalar(value, ctx, frame, rows)?;
                let v = (v.sql, v.ty);
                let mut whens = Vec::new();
                let mut thens = Vec::new();
                for (w, t) in cases {
                    let ws = self.scalar(w, ctx, frame, rows)?;
                    whens.push(if v.0 == "TRUE" {
                        bool_of(&(ws.sql, ws.ty))
                    } else {
                        eq_sql(&v, &(ws.sql, ws.ty))
                    });
                    let ts = self.scalar(t, ctx, frame, rows)?;
                    thens.push((ts.sql, ts.ty));
                }
                if let Some(d) = default {
                    let ds = self.scalar(d, ctx, frame, rows)?;
                    thens.push((ds.sql, ds.ty));
                }
                let (t, ty) = unify(&thens);
                let mut sql = String::from("(CASE");
                for (w, x) in whens.iter().zip(&t) {
                    sql.push_str(&format!(" WHEN {w} THEN {x}"));
                }
                if default.is_some() {
                    sql.push_str(&format!(" ELSE {}", t.last().unwrap()));
                }
                sql.push_str(" END)");
                lit(sql, ty)
            }
            Bound::In { value, list } => {
                let v = self.scalar(value, ctx, frame, rows)?;
                let v = (v.sql, v.ty);
                let mut parts = Vec::new();
                for item in list {
                    let s = self.scalar(item, ctx, frame, rows)?;
                    parts.push(eq_sql(&v, &(s.sql, s.ty)));
                }
                lit(format!("({})", parts.join(" OR ")), ValueType::Bool)
            }
            Bound::Unary { op, expr } => {
                let e = self.scalar(expr, ctx, frame, rows)?;
                let (sql, ty) = unary_sql(*op, &(e.sql, e.ty));
                lit(sql, ty)
            }
            Bound::Binary { op, left, right } => {
                let l = self.scalar(left, ctx, frame, rows)?;
                let r = self.scalar(right, ctx, frame, rows)?;
                let (sql, ty) = binary_sql(*op, &(l.sql, l.ty), &(r.sql, r.ty));
                lit(sql, ty)
            }
        }
    }
}

/// The expression behind a projection: a measure, or a summarised column.
fn projection_bound(p: &Projection) -> Result<Bound> {
    match (&p.field, p.aggregation) {
        (FieldRef::Measure { table, measure }, _) => Ok(Bound::Measure {
            table: table.clone(),
            measure: measure.clone(),
        }),
        (FieldRef::Column { table, column }, Some(a)) => Ok(Bound::Aggregate {
            func: match a {
                Aggregation::Sum => AggFunc::Sum,
                Aggregation::Average => AggFunc::Average,
                Aggregation::Min => AggFunc::Min,
                Aggregation::Max => AggFunc::Max,
                Aggregation::Count => AggFunc::Count,
                Aggregation::DistinctCount => AggFunc::DistinctCount,
            },
            source: TableExpr::Table(table.clone()),
            arg: Some(Box::new(Bound::Column {
                table: table.clone(),
                column: column.clone(),
            })),
        }),
        (FieldRef::Column { .. }, None) => Err(CompileError::Field(
            "a column used as a value needs an aggregation".into(),
        )),
    }
}

pub fn compile(model: &Model, catalog: &dyn Catalog, q: &VisualQuery) -> Result<Compiled> {
    // Keys: the groups, plus hidden sort-by columns that order them.
    let mut keys: Vec<Key> = Vec::new();
    let mut sort_keys: Vec<String> = Vec::new();
    for (i, p) in q.groups.iter().enumerate() {
        let FieldRef::Column { table: t, column } = &p.field else {
            return Err(CompileError::Field("only columns can be grouped by".into()));
        };
        let tab = table(model, t)?;
        let col = tab.column(column).ok_or_else(|| {
            CompileError::Field(format!(
                "a field refers to a missing column of '{}'",
                tab.name
            ))
        })?;
        keys.push(Key {
            name: format!("g{i}"),
            table: t.clone(),
            column: column.clone(),
        });
        sort_keys.push(match &col.sort_by {
            Some(by)
                if tab.column(by).is_some()
                    && !q.groups.iter().any(|g| {
                        g.field
                            == FieldRef::Column {
                                table: t.clone(),
                                column: by.clone(),
                            }
                    }) =>
            {
                format!("s{i}")
            }
            _ => format!("g{i}"),
        });
    }
    let n_groups = keys.len();
    for i in 0..n_groups {
        if sort_keys[i].starts_with('s') {
            let by = table(model, &keys[i].table)?
                .column(&keys[i].column)
                .and_then(|c| c.sort_by.clone())
                .unwrap_or_default();
            keys.push(Key {
                name: format!("s{i}"),
                table: keys[i].table.clone(),
                column: by,
            });
        }
    }

    let mut c = Compiler {
        model,
        catalog,
        ctes: Vec::new(),
        vars: Vec::new(),
        stack: Vec::new(),
        top_keys: keys.clone(),
        top_filters: Vec::new(),
        dom: None,
        dks: Vec::new(),
        counter: 0,
    };

    // Column filters (Top N filters become value lists via a subquery); value filters
    // apply to the final rows.
    let mut filters: Vec<CFilter> = Vec::new();
    let mut value_filters: Vec<(Bound, &FilterKind)> = Vec::new();
    for (i, f) in q.filters.iter().enumerate() {
        match (&f.field.field, f.field.aggregation) {
            (FieldRef::Column { table: t, column }, None) => match &f.filter {
                FilterKind::TopN { count, bottom, by } => {
                    let others: Vec<QueryFilter> = q
                        .filters
                        .iter()
                        .enumerate()
                        .filter(|(j, _)| *j != i)
                        .map(|(_, f)| f.clone())
                        .collect();
                    let sub = compile(
                        model,
                        catalog,
                        &VisualQuery {
                            groups: vec![f.field.clone()],
                            values: vec![by.clone()],
                            filters: others,
                            order: vec![OrderBy {
                                target: OrderTarget::Value(0),
                                descending: !bottom,
                            }],
                            limit: Some(*count),
                            keys: false,
                        },
                    )?;
                    filters.push(CFilter {
                        table: t.clone(),
                        columns: vec![column.clone()],
                        pred: Pred::Sql(
                            column.clone(),
                            format!("CAST(\u{7} AS VARCHAR) IN (SELECT CAST(g0 AS VARCHAR) FROM ({}) AS topn)", sub.sql),
                        ),
                    });
                }
                FilterKind::All => {}
                kind => filters.push(CFilter {
                    table: t.clone(),
                    columns: vec![column.clone()],
                    pred: Pred::Kind(column.clone(), kind.clone()),
                }),
            },
            _ => value_filters.push((projection_bound(&f.field)?, &f.filter)),
        }
    }
    c.top_filters = filters.clone();

    let ctx = Ctx {
        keys: keys.clone(),
        filters,
        use_rels: Vec::new(),
    };
    let frame = Frame {
        rel: "k".into(),
        keys: keys.clone(),
    };
    let values = q
        .values
        .iter()
        .map(|p| projection_bound(p).and_then(|b| c.scalar(&b, &ctx, &frame, &[])))
        .collect::<Result<Vec<_>>>()?;
    let vfilters = value_filters
        .iter()
        .map(|(b, kind)| Ok((c.scalar(b, &ctx, &frame, &[])?, *kind)))
        .collect::<Result<Vec<_>>>()?;

    // The frame's rows: where some top-level aggregate has rows.
    let key_names: Vec<String> = keys.iter().map(|k| k.name.clone()).collect();
    let mut top: BTreeSet<usize> = BTreeSet::new();
    for s in values.iter().chain(vfilters.iter().map(|(s, _)| s)) {
        let mut parts = s.sql.split('\u{1}');
        parts.next();
        while let (Some(idx), Some(_)) = (parts.next(), parts.next()) {
            top.insert(idx.parse().unwrap_or(0));
        }
    }
    let k_sql = if key_names.is_empty() {
        "SELECT 1 AS one".to_string()
    } else if top.is_empty() {
        format!("SELECT * FROM {}", c.domain()?)
    } else {
        let mut parts = Vec::new();
        for i in top {
            let ck = c.ctes[i].keys.clone();
            let name = c.ctes[i].name.clone();
            let exact = c.ctes[i].exact;
            let missing: Vec<&String> = key_names.iter().filter(|k| !ck.contains(k)).collect();
            // Keys the CTE lacks come from each table's key domain: joined on the keys
            // of that table it has (a sort-by column follows its column), crossed when
            // it has none of them.
            let mut part = format!(
                "SELECT {} FROM {name} AS x",
                key_names
                    .iter()
                    .map(|k| format!("{{{k}}}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            let mut source: Vec<(String, String)> =
                ck.iter().map(|k| (k.clone(), format!("x.{k}"))).collect();
            if !missing.is_empty() {
                for (j, (_, dk, names)) in c.key_domains()?.into_iter().enumerate() {
                    let lacking: Vec<&String> = names.iter().filter(|n| !ck.contains(n)).collect();
                    if lacking.is_empty() {
                        continue;
                    }
                    let alias = format!("d{j}");
                    let shared: Vec<&String> = names.iter().filter(|n| ck.contains(n)).collect();
                    if shared.is_empty() {
                        part.push_str(&format!(" CROSS JOIN {dk} AS {alias}"));
                    } else {
                        part.push_str(&format!(
                            " JOIN {dk} AS {alias} ON {}",
                            shared
                                .iter()
                                .map(|n| format!("{alias}.{n} IS NOT DISTINCT FROM x.{n}"))
                                .collect::<Vec<_>>()
                                .join(" AND ")
                        ));
                    }
                    for n in lacking {
                        source.push((n.clone(), format!("{alias}.{n}")));
                    }
                }
            }
            for (k, e) in &source {
                part = part.replace(&format!("{{{k}}}"), e);
            }
            // A context the visual doesn't have (CALCULATE replaced a filter) can produce
            // combinations the visual's own filters exclude: keep the existing ones.
            if !exact {
                let mut conds = Vec::new();
                for (_, dk, names) in c.key_domains()? {
                    let shared: Vec<&String> = names.iter().filter(|n| ck.contains(n)).collect();
                    if shared.is_empty() {
                        continue;
                    }
                    conds.push(format!(
                        "EXISTS (SELECT 1 FROM {dk} AS e WHERE {})",
                        shared
                            .iter()
                            .map(|n| format!("e.{n} IS NOT DISTINCT FROM x.{n}"))
                            .collect::<Vec<_>>()
                            .join(" AND ")
                    ));
                }
                if !conds.is_empty() {
                    part.push_str(&format!(" WHERE {}", conds.join(" AND ")));
                }
            }
            parts.push(part);
        }
        parts.join(" UNION ")
    };

    let mut select: Vec<String> = Vec::new();
    for k in &keys {
        let col_ty = table(model, &k.table)?
            .column(&k.column)
            .map(|c| c.data_type);
        select.push(if matches!(col_ty, Some(DataType::Decimal)) {
            format!("CAST(f.{0} AS DOUBLE) AS {0}", k.name)
        } else {
            format!("f.{0} AS {0}", k.name)
        });
    }
    // Render every value over the same joined frame.
    let mut all = String::new();
    for v in &values {
        all.push_str(&v.sql);
    }
    for (v, _) in &vfilters {
        all.push_str(&v.sql);
    }
    let (_, from) = c.render(
        &Scalar {
            sql: all,
            ty: ValueType::Number,
        },
        "k",
    );
    let mut value_types = Vec::new();
    let mut value_names = Vec::new();
    for (i, v) in values.iter().enumerate() {
        let (sql, _) = c.render(v, "k");
        value_types.push(v.ty);
        value_names.push(format!("v{i}"));
        select.push(if v.ty == ValueType::Number {
            format!("CAST({sql} AS DOUBLE) AS v{i}")
        } else {
            format!("{sql} AS v{i}")
        });
    }
    let mut text_keys = Vec::new();
    if q.keys {
        for (i, k) in keys.iter().take(n_groups).enumerate() {
            select.push(format!("CAST(f.{} AS VARCHAR) AS k{i}", k.name));
            text_keys.push(format!("k{i}"));
        }
    }
    let mut where_: Vec<String> = Vec::new();
    for (v, kind) in &vfilters {
        let (sql, _) = c.render(v, "k");
        if let Some(p) = predicate(&format!("CAST({sql} AS DOUBLE)"), DataType::Double, kind) {
            where_.push(p);
        }
    }
    let mut inner = format!(
        "SELECT {} FROM {from}",
        if select.is_empty() {
            "1 AS one".into()
        } else {
            select.join(", ")
        }
    );
    if !where_.is_empty() {
        inner.push_str(&format!(" WHERE {}", where_.join(" AND ")));
    }

    // CTEs in creation order (which respects their dependencies), the key domain
    // first, and the frame `k` last.
    let mut with: Vec<String> = c
        .ctes
        .iter()
        .map(|x| format!("{} AS ({})", x.name, x.sql))
        .collect();
    with.push(format!("k AS ({k_sql})"));
    // Materialised: DuckDB 2.0 pushes the blank-row filter below into this query and
    // then evaluates some CASE expressions over cross-joined CTEs wrongly (an IF took
    // its ELSE branch for one row only when the filter was there).
    with.push(format!("res AS MATERIALIZED ({inner})"));

    let visible: Vec<String> = keys
        .iter()
        .take(n_groups)
        .map(|k| k.name.clone())
        .chain(value_names.iter().cloned())
        .chain(text_keys.iter().cloned())
        .collect();
    let hidden: Vec<String> = keys.iter().skip(n_groups).map(|k| k.name.clone()).collect();
    let mut outer_cols = visible.clone();
    outer_cols.extend(hidden.iter().cloned());
    let mut sql = format!(
        "WITH {} SELECT {} FROM res AS r",
        with.join(", "),
        if outer_cols.is_empty() {
            "1 AS one".into()
        } else {
            outer_cols.join(", ")
        }
    );
    if !value_names.is_empty() && !key_names.is_empty() {
        sql.push_str(&format!(
            " WHERE {}",
            value_names
                .iter()
                .map(|v| format!("{v} IS NOT NULL"))
                .collect::<Vec<_>>()
                .join(" OR ")
        ));
    }
    let mut order: Vec<String> = q
        .order
        .iter()
        .filter_map(|o| {
            let col = match o.target {
                OrderTarget::Group(i) => sort_keys.get(i)?.clone(),
                OrderTarget::Value(i) => value_names.get(i)?.clone(),
            };
            Some(if o.descending {
                format!("{col} DESC NULLS LAST")
            } else {
                format!("{col} ASC NULLS FIRST")
            })
        })
        .collect();
    for k in &sort_keys {
        if !order.iter().any(|o| o.starts_with(&format!("{k} "))) {
            order.push(format!("{k} ASC NULLS FIRST"));
        }
    }
    if !hidden.is_empty() {
        sql = format!("SELECT {} FROM ({sql}) AS o", visible.join(", "));
    }
    if !order.is_empty() {
        sql.push_str(&format!(" ORDER BY {}", order.join(", ")));
    }
    if let Some(n) = q.limit {
        sql.push_str(&format!(" LIMIT {n}"));
    }
    Ok(Compiled { sql, value_types })
}

/// The SQL that computes a calculated column: one row per row of its table, with the
/// table's `rowid` as `rid` and the value as `v`, and the value's type. Calculated
/// columns may use the row's columns, RELATED lookups and scalar functions.
pub fn calculated_column(
    model: &Model,
    catalog: &dyn Catalog,
    table_id: &str,
    expression: &str,
) -> Result<(String, ValueType)> {
    let (bound, diags) = charted_dax::check_in(model, expression, Some(table_id));
    let bound = bound.ok_or_else(|| {
        CompileError::Field(
            diags
                .first()
                .map(|d| d.message.clone())
                .unwrap_or_else(|| "the expression has an error".into()),
        )
    })?;
    if !row_pure(&bound) {
        return Err(CompileError::Unsupported(
            "measures, aggregates and CALCULATE in calculated columns".into(),
        ));
    }
    let mut c = Compiler {
        model,
        catalog,
        ctes: Vec::new(),
        vars: Vec::new(),
        stack: Vec::new(),
        top_keys: Vec::new(),
        top_filters: Vec::new(),
        dom: None,
        dks: Vec::new(),
        counter: 0,
    };
    let mut related = Vec::new();
    related_refs(&bound, &mut related);
    let extra: Vec<Id> = related.into_iter().map(|(t, _)| t).collect();
    let (_, _, from, _) = c.scope(table_id, &Ctx::default(), &extra, false)?;
    let (v, ty) = c.row(&bound, None)?;
    Ok((
        format!(
            "SELECT {}.rowid AS rid, {v} AS v FROM {from}",
            alias(model, table_id)
        ),
        ty,
    ))
}

/// The model type a calculated column's values get.
pub fn value_type_to_data_type(t: ValueType) -> DataType {
    match t {
        ValueType::Number => DataType::Double,
        ValueType::Text => DataType::String,
        ValueType::Bool => DataType::Boolean,
        ValueType::Date => DataType::Date,
    }
}
