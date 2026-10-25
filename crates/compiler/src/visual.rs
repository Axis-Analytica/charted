//! Compiles a visual's query (grouping columns, values, filters, order) to one DuckDB
//! SELECT with Power BI's semantics:
//!
//! - Measures are DAX. Referenced measures are expanded inline; every aggregate in the
//!   expanded expressions is computed over its own table, so one visual can mix
//!   measures from different fact tables.
//! - Aggregates are grouped by the table they aggregate. Each such group becomes a CTE
//!   over that table joined (left, many-to-one) to the tables it reaches through active
//!   relationships; it groups by the grouping columns it can reach and applies the
//!   filters it can reach. Unreachable groups and filters do not affect it, which is
//!   DAX's single-direction filter propagation (its values repeat across those groups).
//! - The CTEs are combined on the grouping keys with null-safe joins, and rows where
//!   every value is blank are dropped, like SUMMARIZECOLUMNS.
//! - BLANK follows DAX: `+`/`-` treat it as zero unless both sides are blank, DIVIDE by
//!   zero or blank gives the alternate result, counts of nothing are blank.

use charted_dax::{AggFunc, BinaryOp, Bound, ScalarFunc, UnaryOp};
use charted_model::{
    Aggregation, ColumnKind, Condition, DataType, FieldRef, FilterKind, FilterOp, Id, Model,
    Projection, Table,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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

/// A scalar expression over aggregates, before the aggregates are placed in CTEs.
#[derive(Debug, Clone)]
enum Scalar {
    Lit(String, ValueType),
    Agg(usize, ValueType),
    Raw(String, ValueType),
}

/// An aggregate over one table: the SQL is in terms of that table's alias.
#[derive(Debug, Clone, PartialEq)]
struct Agg {
    table: Id,
    sql: String,
}

struct Ctx<'a> {
    model: &'a Model,
    catalog: &'a dyn Catalog,
    aggs: Vec<Agg>,
}

/// SQL alias of a table inside a CTE.
fn alias(model: &Model, table: &str) -> String {
    let i = model.tables.iter().position(|t| t.id == table).unwrap_or(0);
    format!("t{i}")
}

fn table<'a>(model: &'a Model, id: &str) -> Result<&'a Table> {
    model
        .table(id)
        .ok_or_else(|| CompileError::Field(format!("a field refers to a missing table ({id})")))
}

impl<'a> Ctx<'a> {
    /// A column's value in its model type, over the table's alias.
    fn column_sql(&self, table_id: &str, column: &str) -> Result<(String, ValueType)> {
        let t = table(self.model, table_id)?;
        let c = t.column(column).ok_or_else(|| {
            CompileError::Field(format!(
                "a field refers to a missing column of '{}'",
                t.name
            ))
        })?;
        let ColumnKind::Data { source_column } = &c.kind else {
            return Err(CompileError::Field(format!(
                "'{}'[{}] is a calculated column, which isn't supported yet",
                t.name, c.name
            )));
        };
        let raw = format!("{}.{}", alias(self.model, table_id), ident(source_column));
        let physical = self.catalog.physical_type(&t.source, source_column);
        let sql = match (physical, crate::sqltype(c.data_type)) {
            (Some(p), Some(target)) if p != c.data_type => format!("TRY_CAST({raw} AS {target})"),
            _ => raw,
        };
        Ok((sql, ValueType::of(c.data_type)))
    }

    fn add_agg(&mut self, table: &str, sql: String, ty: ValueType) -> Scalar {
        let agg = Agg {
            table: table.to_string(),
            sql,
        };
        let i = match self.aggs.iter().position(|a| *a == agg) {
            Some(i) => i,
            None => {
                self.aggs.push(agg);
                self.aggs.len() - 1
            }
        };
        Scalar::Agg(i, ty)
    }

    fn projection(&mut self, p: &Projection) -> Result<Scalar> {
        match (&p.field, p.aggregation) {
            (FieldRef::Measure { measure, .. }, _) => self.measure(measure, &mut Vec::new()),
            (FieldRef::Column { table, column }, Some(agg)) => {
                let (col, ty) = self.column_sql(table, column)?;
                let func = match agg {
                    Aggregation::Sum => AggFunc::Sum,
                    Aggregation::Average => AggFunc::Average,
                    Aggregation::Min => AggFunc::Min,
                    Aggregation::Max => AggFunc::Max,
                    Aggregation::Count => AggFunc::Count,
                    Aggregation::DistinctCount => AggFunc::DistinctCount,
                };
                let (sql, ty) = agg_sql(func, Some((col, ty)));
                Ok(self.add_agg(table, sql, ty))
            }
            (FieldRef::Column { .. }, None) => Err(CompileError::Field(
                "a column used as a value needs an aggregation".into(),
            )),
        }
    }

    fn measure(&mut self, id: &str, stack: &mut Vec<Id>) -> Result<Scalar> {
        let (_, m) = self
            .model
            .tables
            .iter()
            .find_map(|t| t.measures.iter().find(|m| m.id == id).map(|m| (t, m)))
            .ok_or_else(|| {
                CompileError::Field(format!("a field refers to a missing measure ({id})"))
            })?;
        if stack.iter().any(|s| s == id) {
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
        stack.push(id.to_string());
        let s = self.scalar(&bound, stack);
        stack.pop();
        s
    }

    /// Lowers a measure-level expression (outside any row context).
    fn scalar(&mut self, b: &Bound, stack: &mut Vec<Id>) -> Result<Scalar> {
        Ok(match b {
            Bound::Number(n) => Scalar::Lit(number(*n), ValueType::Number),
            Bound::Text(s) => Scalar::Lit(string(s), ValueType::Text),
            Bound::Bool(v) => Scalar::Lit(v.to_string().to_uppercase(), ValueType::Bool),
            Bound::Blank => Scalar::Lit("NULL".into(), ValueType::Number),
            Bound::Measure { measure, .. } => self.measure(measure, stack)?,
            Bound::Column { .. } => {
                return Err(CompileError::Field(
                    "a column needs a row context here".into(),
                ));
            }
            Bound::Aggregate { func, table, arg } => {
                let row = match arg {
                    Some(a) => Some(self.row(a)?),
                    None => None,
                };
                let (sql, ty) = agg_sql(*func, row);
                self.add_agg(table, sql, ty)
            }
            Bound::Call { func, args } => {
                let args = args
                    .iter()
                    .map(|a| self.scalar(a, stack))
                    .collect::<Result<Vec<_>>>()?;
                let (sql, ty) = call_sql(*func, &args.iter().map(render_raw).collect::<Vec<_>>());
                Scalar::Raw(sql, ty)
            }
            Bound::Unary { op, expr } => {
                let e = self.scalar(expr, stack)?;
                let (sql, ty) = unary_sql(*op, &render_raw(&e));
                Scalar::Raw(sql, ty)
            }
            Bound::Binary { op, left, right } => {
                let l = render_raw(&self.scalar(left, stack)?);
                let r = render_raw(&self.scalar(right, stack)?);
                let (sql, ty) = binary_sql(*op, &l, &r);
                Scalar::Raw(sql, ty)
            }
        })
    }

    /// Lowers a row expression (an aggregate's argument) over its table's alias.
    fn row(&mut self, b: &Bound) -> Result<(String, ValueType)> {
        Ok(match b {
            Bound::Number(n) => (number(*n), ValueType::Number),
            Bound::Text(s) => (string(s), ValueType::Text),
            Bound::Bool(v) => (v.to_string().to_uppercase(), ValueType::Bool),
            Bound::Blank => ("NULL".into(), ValueType::Number),
            Bound::Column { table, column } => self.column_sql(table, column)?,
            Bound::Call { func, args } => {
                let args = args
                    .iter()
                    .map(|a| self.row(a))
                    .collect::<Result<Vec<_>>>()?;
                call_sql(*func, &args)
            }
            Bound::Unary { op, expr } => unary_sql(*op, &self.row(expr)?),
            Bound::Binary { op, left, right } => {
                binary_sql(*op, &self.row(left)?, &self.row(right)?)
            }
            Bound::Measure { .. } | Bound::Aggregate { .. } => {
                return Err(CompileError::Field(
                    "aggregates inside iterators aren't supported yet".into(),
                ));
            }
        })
    }
}

/// Renders a scalar as SQL, with aggregate `i` as the placeholder `\u{1}i\u{1}`.
fn render_raw(s: &Scalar) -> (String, ValueType) {
    match s {
        Scalar::Lit(sql, t) | Scalar::Raw(sql, t) => (sql.clone(), *t),
        Scalar::Agg(i, t) => (format!("\u{1}{i}\u{1}"), *t),
    }
}

fn number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{n:.0}")
    } else {
        format!("{n}")
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
            format!("nullif(count(*) FILTER (WHERE {x} IS NULL), 0)"),
            ValueType::Number,
        ),
        // DISTINCTCOUNT counts blank as a value.
        AggFunc::DistinctCount => (
            format!("nullif(count(DISTINCT {x}) + coalesce(max(({x} IS NULL)::INTEGER), 0), 0)"),
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
        ScalarFunc::Round => (format!("round({}, {})", a[0].0, a[1].0), num),
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
    }
}

/// A value used as a condition: numbers are true when non-zero, as in DAX.
fn bool_of(x: &(String, ValueType)) -> String {
    match x.1 {
        ValueType::Bool => x.0.clone(),
        ValueType::Number => format!("(({}) <> 0)", x.0),
        _ => format!("(({}) IS NOT NULL)", x.0),
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
    // Blank compares as zero or empty text.
    let blank = |x: &(String, ValueType)| match x.1 {
        ValueType::Number => format!("coalesce({}, 0)", x.0),
        ValueType::Text => format!("coalesce({}, '')", x.0),
        ValueType::Bool => format!("coalesce({}, FALSE)", x.0),
        ValueType::Date => x.0.clone(),
    };
    let cmp = |sym: &str| {
        (
            format!("({} {sym} {})", blank(l), blank(r)),
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
        BinaryOp::Div => (format!("(({a}) / ({b}))"), ValueType::Number),
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

/// A grouping column, resolved.
struct GroupCol {
    table: Id,
    column: Id,
    /// Output name: `g0…` for requested groups, `s0…` for hidden sort keys.
    name: String,
}

pub fn compile(model: &Model, catalog: &dyn Catalog, q: &VisualQuery) -> Result<Compiled> {
    let mut ctx = Ctx {
        model,
        catalog,
        aggs: Vec::new(),
    };

    // Groups, plus hidden sort-by columns that order them.
    let mut groups: Vec<GroupCol> = Vec::new();
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
        groups.push(GroupCol {
            table: t.clone(),
            column: column.clone(),
            name: format!("g{i}"),
        });
        sort_keys.push(match &col.sort_by {
            Some(by) if tab.column(by).is_some() => format!("s{i}"),
            _ => format!("g{i}"),
        });
    }
    let n_groups = groups.len();
    for i in 0..n_groups {
        if sort_keys[i].starts_with('s') {
            let tab = table(model, &groups[i].table)?;
            let by = tab
                .column(&groups[i].column)
                .and_then(|c| c.sort_by.clone())
                .unwrap_or_default();
            groups.push(GroupCol {
                table: groups[i].table.clone(),
                column: by,
                name: format!("s{i}"),
            });
        }
    }

    // Values: scalars over aggregates.
    let values = q
        .values
        .iter()
        .map(|p| ctx.projection(p))
        .collect::<Result<Vec<_>>>()?;

    // Column filters (Top N filters become value-list filters via a subquery); value
    // filters apply to the final rows.
    let mut col_filters: Vec<(Id, String)> = Vec::new();
    let mut value_filters: Vec<(Scalar, &FilterKind)> = Vec::new();
    for (i, f) in q.filters.iter().enumerate() {
        match (&f.field.field, f.field.aggregation) {
            (FieldRef::Column { table: t, column }, None) => {
                let (e, _) = ctx.column_sql(t, column)?;
                let ty = table(model, t)?
                    .column(column)
                    .map(|c| c.data_type)
                    .unwrap_or(DataType::String);
                let pred = match &f.filter {
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
                        Some(format!(
                            "CAST({e} AS VARCHAR) IN (SELECT CAST(g0 AS VARCHAR) FROM ({}) AS topn)",
                            sub.sql
                        ))
                    }
                    kind => predicate(&e, ty, kind),
                };
                if let Some(p) = pred {
                    col_filters.push((t.clone(), p));
                }
            }
            _ => {
                if f.field.aggregation.is_none() && matches!(f.field.field, FieldRef::Column { .. })
                {
                    continue;
                }
                let s = ctx.projection(&f.field)?;
                value_filters.push((s, &f.filter));
            }
        }
    }

    // One CTE per aggregated table.
    let mut by_table: BTreeMap<Id, Vec<usize>> = BTreeMap::new();
    for (i, a) in ctx.aggs.iter().enumerate() {
        by_table.entry(a.table.clone()).or_default().push(i);
    }
    let mut ctes: Vec<String> = Vec::new();
    let mut cte_keys: Vec<Vec<String>> = Vec::new();
    // Where each aggregate ends up: (cte index, column name).
    let mut agg_at: Vec<(usize, String)> = vec![(0, String::new()); ctx.aggs.len()];
    for (base, aggs) in &by_table {
        let reach = Reach::from(model, base);
        let keys: Vec<&GroupCol> = groups
            .iter()
            .filter(|g| reach.reaches(base, &g.table))
            .collect();
        let filters: Vec<&(Id, String)> = col_filters
            .iter()
            .filter(|(t, _)| reach.reaches(base, t))
            .collect();
        let mut needed: Vec<Id> = keys.iter().map(|g| g.table.clone()).collect();
        needed.extend(filters.iter().map(|(t, _)| t.clone()));
        let from = from_clause(model, &reach, base, &needed)?;
        let mut select: Vec<String> = Vec::new();
        for g in &keys {
            let (e, _) = ctx.column_sql(&g.table, &g.column)?;
            select.push(format!("{e} AS {}", g.name));
        }
        let ci = ctes.len();
        for &a in aggs {
            select.push(format!("{} AS a{a}", ctx.aggs[a].sql));
            agg_at[a] = (ci, format!("a{a}"));
        }
        let mut sql = format!("SELECT {} FROM {from}", select.join(", "));
        if !filters.is_empty() {
            sql.push_str(&format!(
                " WHERE {}",
                filters
                    .iter()
                    .map(|(_, p)| p.as_str())
                    .collect::<Vec<_>>()
                    .join(" AND ")
            ));
        }
        if !keys.is_empty() {
            sql.push_str(&format!(
                " GROUP BY {}",
                (1..=keys.len())
                    .map(|i| i.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        ctes.push(sql);
        cte_keys.push(keys.iter().map(|g| g.name.clone()).collect());
    }

    let key_names: Vec<String> = groups.iter().map(|g| g.name.clone()).collect();
    let mut with: Vec<String> = ctes
        .iter()
        .enumerate()
        .map(|(i, s)| format!("f{i} AS ({s})"))
        .collect();

    // The rows: the union of the keys of CTEs that have them all; otherwise distinct
    // group values (by table) cross joined.
    let full: Vec<usize> = (0..ctes.len())
        .filter(|&i| cte_keys[i].len() == key_names.len())
        .collect();
    let keys_sql = if key_names.is_empty() {
        None
    } else if !full.is_empty() {
        Some(
            full.iter()
                .map(|i| format!("SELECT {} FROM f{i}", key_names.join(", ")))
                .collect::<Vec<_>>()
                .join(" UNION "),
        )
    } else {
        Some(distinct_groups(&ctx, &groups, &col_filters)?)
    };
    let from_rows = match &keys_sql {
        Some(k) => {
            with.push(format!("k AS ({k})"));
            let mut from = "k".to_string();
            for (i, keys) in cte_keys.iter().enumerate() {
                let on: Vec<String> = keys
                    .iter()
                    .map(|n| format!("k.{n} IS NOT DISTINCT FROM f{i}.{n}"))
                    .collect();
                from.push_str(&if on.is_empty() {
                    format!(" CROSS JOIN f{i}")
                } else {
                    format!(" LEFT JOIN f{i} ON {}", on.join(" AND "))
                });
            }
            from
        }
        None if ctes.is_empty() => "(SELECT 1) AS one".into(),
        None => (0..ctes.len())
            .map(|i| format!("f{i}"))
            .collect::<Vec<_>>()
            .join(" CROSS JOIN "),
    };

    let place = |s: &Scalar| -> (String, ValueType) {
        let (sql, ty) = render_raw(s);
        let mut out = String::new();
        let mut parts = sql.split('\u{1}');
        if let Some(first) = parts.next() {
            out.push_str(first);
        }
        while let (Some(idx), Some(rest)) = (parts.next(), parts.next()) {
            let i: usize = idx.parse().unwrap_or(0);
            let (c, name) = &agg_at[i];
            out.push_str(&format!("f{c}.{name}"));
            out.push_str(rest);
        }
        (out, ty)
    };

    let mut select: Vec<String> = Vec::new();
    for g in &groups {
        let col_ty = table(model, &g.table)?
            .column(&g.column)
            .map(|c| c.data_type);
        let src = if keys_sql.is_some() {
            format!("k.{}", g.name)
        } else {
            g.name.clone()
        };
        select.push(if matches!(col_ty, Some(DataType::Decimal)) {
            format!("CAST({src} AS DOUBLE) AS {}", g.name)
        } else {
            format!("{src} AS {}", g.name)
        });
    }
    let mut value_types = Vec::new();
    let mut value_names = Vec::new();
    for (i, v) in values.iter().enumerate() {
        let (sql, ty) = place(v);
        value_types.push(ty);
        value_names.push(format!("v{i}"));
        select.push(if ty == ValueType::Number {
            format!("CAST({sql} AS DOUBLE) AS v{i}")
        } else {
            format!("{sql} AS v{i}")
        });
    }
    let mut where_: Vec<String> = Vec::new();
    for (s, kind) in &value_filters {
        let (sql, _) = place(s);
        if let Some(p) = predicate(&format!("CAST({sql} AS DOUBLE)"), DataType::Double, kind) {
            where_.push(p);
        }
    }
    let mut inner = format!("SELECT {} FROM {from_rows}", select.join(", "));
    if !where_.is_empty() {
        inner.push_str(&format!(" WHERE {}", where_.join(" AND ")));
    }

    // Drop rows where every value is blank.
    let mut outer = format!("SELECT {} FROM ({inner}) AS r", {
        let mut cols: Vec<String> = groups
            .iter()
            .take(n_groups)
            .map(|g| g.name.clone())
            .collect();
        cols.extend(value_names.iter().cloned());
        cols.join(", ")
    });
    if !value_names.is_empty() && !key_names.is_empty() {
        outer.push_str(&format!(
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
    if order.is_empty() {
        order = sort_keys
            .iter()
            .map(|k| format!("{k} ASC NULLS FIRST"))
            .collect();
    }
    // Ties are broken by every key so results are stable.
    for k in &sort_keys {
        let asc = format!("{k} ASC NULLS FIRST");
        if !order.iter().any(|o| o.starts_with(&format!("{k} "))) {
            order.push(asc);
        }
    }
    // Hidden sort keys must be selected to be ordered by.
    if sort_keys.iter().any(|k| k.starts_with('s')) {
        let extra: Vec<String> = sort_keys
            .iter()
            .filter(|k| k.starts_with('s'))
            .cloned()
            .collect();
        outer = outer.replacen("SELECT ", &format!("SELECT {}, ", extra.join(", ")), 1);
        let visible: Vec<String> = groups
            .iter()
            .take(n_groups)
            .map(|g| g.name.clone())
            .chain(value_names.iter().cloned())
            .collect();
        let mut sql = format!(
            "WITH {} SELECT {} FROM ({outer}) AS o",
            with.join(", "),
            visible.join(", ")
        );
        if !order.is_empty() {
            sql.push_str(&format!(" ORDER BY {}", order.join(", ")));
        }
        if let Some(n) = q.limit {
            sql.push_str(&format!(" LIMIT {n}"));
        }
        return Ok(Compiled { sql, value_types });
    }
    let mut sql = if with.is_empty() {
        outer
    } else {
        format!("WITH {} {outer}", with.join(", "))
    };
    if !order.is_empty() {
        sql.push_str(&format!(" ORDER BY {}", order.join(", ")));
    }
    if let Some(n) = q.limit {
        sql.push_str(&format!(" LIMIT {n}"));
    }
    Ok(Compiled { sql, value_types })
}

/// `source AS alias` for `base`, left-joined to every table in `needed` along its path.
fn from_clause(model: &Model, reach: &Reach, base: &str, needed: &[Id]) -> Result<String> {
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
            let col = |tid: &str, cid: &str| -> Result<String> {
                let tab = table(model, tid)?;
                let c = tab.column(cid).ok_or_else(|| {
                    CompileError::Field(format!(
                        "a relationship refers to a missing column of '{}'",
                        tab.name
                    ))
                })?;
                let ColumnKind::Data { source_column } = &c.kind else {
                    return Err(CompileError::Field(
                        "relationships on calculated columns aren't supported yet".into(),
                    ));
                };
                Ok(format!("{}.{}", alias(model, tid), ident(source_column)))
            };
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

/// Distinct group values when no aggregate reaches every group: per table, the values
/// that exist together (with the filters that reach that table), cross joined.
fn distinct_groups(ctx: &Ctx, groups: &[GroupCol], filters: &[(Id, String)]) -> Result<String> {
    let mut tables: Vec<Id> = Vec::new();
    for g in groups {
        if !tables.contains(&g.table) {
            tables.push(g.table.clone());
        }
    }
    let mut parts = Vec::new();
    for t in &tables {
        let reach = Reach::from(ctx.model, t);
        let fs: Vec<&(Id, String)> = filters
            .iter()
            .filter(|(ft, _)| reach.reaches(t, ft))
            .collect();
        let mut needed: Vec<Id> = fs.iter().map(|(ft, _)| ft.clone()).collect();
        needed.push(t.clone());
        let from = from_clause(ctx.model, &reach, t, &needed)?;
        let cols = groups
            .iter()
            .filter(|g| &g.table == t)
            .map(|g| {
                ctx.column_sql(&g.table, &g.column)
                    .map(|(e, _)| format!("{e} AS {}", g.name))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut sql = format!("SELECT DISTINCT {} FROM {from}", cols.join(", "));
        if !fs.is_empty() {
            sql.push_str(&format!(
                " WHERE {}",
                fs.iter()
                    .map(|(_, p)| p.as_str())
                    .collect::<Vec<_>>()
                    .join(" AND ")
            ));
        }
        parts.push(format!("({sql})"));
    }
    Ok(format!(
        "SELECT {} FROM {}",
        groups
            .iter()
            .map(|g| g.name.clone())
            .collect::<Vec<_>>()
            .join(", "),
        parts.join(" CROSS JOIN ")
    ))
}
