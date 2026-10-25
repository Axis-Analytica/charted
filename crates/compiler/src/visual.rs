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

use charted_dax::{AggFunc, BinaryOp, Bound, FilterArg, InfoFunc, ScalarFunc, TableExpr, UnaryOp};
use charted_model::{
    Aggregation, ColumnKind, Condition, DataType, FieldRef, FilterKind, FilterOp, Id, Model,
    Projection, Table,
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
/// filter predicates and its FROM clause.
type Scope = (Vec<(Key, String)>, Vec<String>, String);

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
    columns: Vec<(Id, String)>,
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

fn call_sql(func: ScalarFunc, a: &[(String, ValueType)]) -> (String, ValueType) {
    let num = ValueType::Number;
    match func {
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
    }
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
    fn joins(&self, reach: &Reach, base: &str, needed: &[Id]) -> Result<String> {
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
            }
        }
        Ok(sql)
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
            needed.extend(Self::pred_tables(f));
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
        let mut from = self.joins(&reach, base, &needed)?;
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
        Ok((out_keys, preds, from))
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
            TableExpr::Filter { .. } => unreachable!("filters were unwrapped"),
        };
        let mut related: Vec<(Id, Id)> = Vec::new();
        if let Some(a) = arg {
            related_refs(a, &mut related);
        }
        for c in &conds {
            related_refs(c, &mut related);
        }
        let extra: Vec<Id> = related.iter().map(|(t, _)| t.clone()).collect();
        let (keys, mut preds, from) = self.scope(&base, ctx, &extra, apply_ctx)?;
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
        let exact = self.is_visual_ctx(ctx);
        let i = self.add_cte(sql, key_names, exact);
        let ty = match (func, arg) {
            (AggFunc::Min | AggFunc::Max, Some(a)) => self.row_type(a),
            _ => ValueType::Number,
        };
        Ok(Scalar {
            sql: format!("\u{1}{i}\u{1}"),
            ty,
        })
    }

    fn row_type(&self, b: &Bound) -> ValueType {
        match b {
            Bound::Column { table, column } | Bound::Related { table, column } => {
                ValueType::of(self.column_type(table, column))
            }
            Bound::Text(_) => ValueType::Text,
            Bound::Bool(_) => ValueType::Bool,
            _ => ValueType::Number,
        }
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
                call_sql(*func, &args)
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
            let (_, preds, from) = self.scope(t, &ctx, std::slice::from_ref(t), true)?;
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
        rows: &[RowBinding],
    ) -> Result<Ctx> {
        // Filter arguments see the context before transition; the result starts after it.
        let outer = ctx.clone();
        let mut c = self.transition(ctx, rows);
        // New filters, evaluated in the outer context.
        let mut added: Vec<(CFilter, bool)> = Vec::new();
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
                FilterArg::Table(t) => self.table_filter(t, keep, &outer, &mut added)?,
                FilterArg::Keep(_) => {
                    return Err(CompileError::Unsupported(
                        "KEEPFILTERS inside KEEPFILTERS".into(),
                    ));
                }
            }
        }
        for (f, keep) in added {
            if !keep && !f.columns.is_empty() {
                c.keys.retain(|k| !f.columns.contains(&k.column));
                c.filters
                    .retain(|x| !x.columns.iter().any(|col| f.columns.contains(col)));
            }
            c.filters.push(f);
        }
        Ok(c)
    }

    /// The values of some columns visible in a context (VALUES), as a value set
    /// filter: one row per visible tuple and per key of the context the table reaches.
    fn value_set(&mut self, table_id: &str, columns: &[Id], outer: &Ctx) -> Result<CFilter> {
        let (keys, preds, from) = self.scope(table_id, outer, &[table_id.to_string()], true)?;
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
        added: &mut Vec<(CFilter, bool)>,
    ) -> Result<()> {
        match t {
            TableExpr::Filter { source, condition } if row_pure(condition) => match &**source {
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
                _ => {
                    return Err(CompileError::Unsupported(
                        "this FILTER as a CALCULATE filter".into(),
                    ));
                }
            },
            TableExpr::Filter { .. } => {
                return Err(CompileError::Unsupported(
                    "FILTER with measures or aggregates in its condition, used as a CALCULATE filter,".into(),
                ));
            }
            TableExpr::Values { table, columns } => {
                added.push((self.value_set(table, columns, outer)?, keep))
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
        }
        Ok(())
    }

    /// Context transition: the current rows become keys.
    fn transition(&self, ctx: &Ctx, rows: &[RowBinding]) -> Ctx {
        let mut c = ctx.clone();
        for r in rows {
            for (col, name) in &r.columns {
                c.keys.retain(|k| &k.column != col);
                c.filters.retain(|f| !f.columns.contains(col));
                c.keys.push(Key {
                    name: name.clone(),
                    table: r.table.clone(),
                    column: col.clone(),
                });
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
        let mut conds: Vec<&Bound> = Vec::new();
        let mut cur = source;
        while let TableExpr::Filter { source, condition } = cur {
            conds.push(condition);
            cur = source;
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
            TableExpr::Filter { .. } => unreachable!("filters were unwrapped"),
        };
        let tab = table(self.model, &t)?;
        // A row is identified by the table's key column when it has one.
        let cols: Vec<Id> = match cols {
            Some(c) => c,
            None => match tab.columns.iter().find(|c| c.is_key) {
                Some(k) => vec![k.id.clone()],
                None => tab.columns.iter().map(|c| c.id.clone()).collect(),
            },
        };
        let mut related = Vec::new();
        related_refs(arg, &mut related);
        for c in &conds {
            related_refs(c, &mut related);
        }
        let extra: Vec<Id> = related.iter().map(|(tb, _)| tb.clone()).collect();
        let (keys, mut preds, from) = self.scope(&t, ctx, &extra, apply_ctx)?;
        // Keys of the outer frame this table reaches join the rows to the frame.
        let reach_keys: Vec<(Key, String)> = keys
            .iter()
            .filter(|(k, _)| frame.keys.iter().any(|f| f.name == k.name))
            .cloned()
            .collect();
        let mut binding = RowBinding {
            table: t.clone(),
            columns: Vec::new(),
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
            if row_pure(c) {
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
        // The new frame: outer frame rows joined to the iterated rows.
        let row_cols: Vec<String> = binding
            .columns
            .iter()
            .map(|(_, n)| n.clone())
            .chain(binding.related.iter().map(|(_, n)| n.clone()))
            .collect();
        let join = if reach_keys.is_empty() {
            format!("CROSS JOIN {rows_name} AS r")
        } else {
            format!(
                "JOIN {rows_name} AS r ON {}",
                reach_keys
                    .iter()
                    .map(|(k, _)| format!("o.{0} IS NOT DISTINCT FROM r.{0}", k.name))
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
        let outer_cols: Vec<String> = frame.keys.iter().map(|k| format!("o.{}", k.name)).collect();
        let mut fsel = outer_cols.clone();
        fsel.extend(row_cols.iter().map(|c| format!("r.{c}")));
        let fsel = if fsel.is_empty() {
            "1 AS one".to_string()
        } else {
            fsel.join(", ")
        };
        self.ctes.push(CteDef {
            name: frame_name.clone(),
            sql: format!("SELECT {fsel} FROM {outer_rel} AS o {join}"),
            keys: Vec::new(),
            exact: true,
        });
        let mut inner_keys = frame.keys.clone();
        for (c, n) in &binding.columns {
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
        let mut filter_sql = Vec::new();
        for c in later {
            let s = self.scalar(c, ctx, &inner_frame, &inner_rows)?;
            filter_sql.push(bool_of(&(s.sql, s.ty)));
        }
        let value = self.scalar(arg, ctx, &inner_frame, &inner_rows)?;
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
            Bound::Aggregate { func, source, arg } => {
                let pure = arg.as_deref().is_none_or(row_pure) && {
                    let mut ok = true;
                    let mut t = source;
                    while let TableExpr::Filter { source, condition } = t {
                        ok &= row_pure(condition);
                        t = source;
                    }
                    ok
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
                let c = self.calculate_ctx(ctx, filters, rows)?;
                self.scalar(expr, &c, frame, rows)
            }
            Bound::Call { func, args } => {
                let a = args
                    .iter()
                    .map(|x| self.scalar(x, ctx, frame, rows).map(|s| (s.sql, s.ty)))
                    .collect::<Result<Vec<_>>>()?;
                let (sql, ty) = call_sql(*func, &a);
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

    let visible: Vec<String> = keys
        .iter()
        .take(n_groups)
        .map(|k| k.name.clone())
        .chain(value_names.iter().cloned())
        .collect();
    let hidden: Vec<String> = keys.iter().skip(n_groups).map(|k| k.name.clone()).collect();
    let mut outer_cols = visible.clone();
    outer_cols.extend(hidden.iter().cloned());
    let mut sql = format!(
        "WITH {} SELECT {} FROM ({inner}) AS r",
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
