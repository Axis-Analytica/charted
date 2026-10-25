//! A reference DAX interpreter: evaluates visual queries directly over in-memory rows
//! with textbook filter-context semantics. It is slow and simple on purpose; tests use
//! it as the oracle the SQL compiler must agree with.
//!
//! Semantics (shared with the compiler):
//! - The filter context is a list of filters, each a set of allowed value tuples over
//!   columns of one table. A row of table T is visible when every filter on T matches
//!   it and every filter on a table T reaches (many → one along active relationships)
//!   matches its related row (a missing related row counts as all-blank).
//! - A visual row adds one single-value filter per grouping column (marked grouping).
//! - CALCULATE: filter arguments are evaluated in the outer context; then the current
//!   rows become filters (context transition); then ALL/REMOVEFILTERS/ALLEXCEPT/
//!   ALLSELECTED remove filters; then each filter argument replaces the filters that
//!   touch its columns (KEEPFILTERS adds without replacing).
//! - A measure reference is CALCULATE ( expression ).
//! - SUMMARIZECOLUMNS: rows are the distinct visible tuples of each table's grouping
//!   columns, cross joined, minus rows where every value is blank.

use charted_dax::{
    AggFunc, BinaryOp, Bound, DateOp, FilterArg, InfoFunc, Period, ScalarFunc, TableExpr, UnaryOp,
};
use charted_model::{
    Aggregation, Cardinality, Condition, FieldRef, FilterKind, FilterOp, Id, Model, Projection,
};
use std::collections::{HashMap, HashSet};

use crate::visual::{OrderTarget, VisualQuery};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Blank,
    Bool(bool),
    Num(f64),
    Text(String),
}

impl Value {
    pub fn is_blank(&self) -> bool {
        matches!(self, Value::Blank)
    }

    /// The value as DuckDB's CAST(… AS VARCHAR) writes it (what filters compare).
    pub fn key(&self) -> Option<String> {
        match self {
            Value::Blank => None,
            Value::Bool(b) => Some(b.to_string()),
            Value::Num(n) => Some(if n.fract() == 0.0 && n.abs() < 1e15 {
                format!("{n:.0}")
            } else {
                n.to_string()
            }),
            Value::Text(s) => Some(s.clone()),
        }
    }

    fn num(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    fn truthy(&self) -> bool {
        match self {
            Value::Blank => false,
            Value::Bool(b) => *b,
            Value::Num(n) => *n != 0.0,
            Value::Text(s) => s.eq_ignore_ascii_case("true"),
        }
    }
}

fn cmp(a: &Value, b: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (Value::Blank, Value::Blank) => Ordering::Equal,
        (Value::Blank, _) => Ordering::Less,
        (_, Value::Blank) => Ordering::Greater,
        _ => match (a.num(), b.num()) {
            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
            _ => a
                .key()
                .unwrap_or_default()
                .to_lowercase()
                .cmp(&b.key().unwrap_or_default().to_lowercase()),
        },
    }
}

/// Blank compared as zero, empty text or false (DAX's `=`, `<`…).
fn coerced(v: &Value, like: &Value) -> Value {
    match (v, like) {
        (Value::Blank, Value::Text(_)) => Value::Text(String::new()),
        (Value::Blank, Value::Bool(_)) => Value::Bool(false),
        (Value::Blank, _) => Value::Num(0.0),
        _ => v.clone(),
    }
}

pub struct TableData {
    pub columns: Vec<Id>,
    pub rows: Vec<Vec<Value>>,
}

impl TableData {
    fn col(&self, id: &str) -> Option<usize> {
        self.columns.iter().position(|c| c == id)
    }
}

/// Rows of every model table, by table id, with columns by model column id.
pub struct Data {
    pub tables: HashMap<Id, TableData>,
}

type Tuple = Vec<Option<String>>;

#[derive(Clone, Debug)]
struct Filter {
    table: Id,
    columns: Vec<Id>,
    tuples: HashSet<Tuple>,
    /// A visual grouping or an iterator's row (what ALLSELECTED removes).
    grouping: bool,
}

#[derive(Clone, Debug, Default)]
struct Ctx {
    filters: Vec<Filter>,
    /// Relationships switched on by USERELATIONSHIP.
    use_rels: Vec<Id>,
}

/// A row of an iterator: its table and the values it has.
#[derive(Clone, Debug)]
struct Row {
    table: Id,
    values: HashMap<Id, Value>,
}

struct Interp<'a> {
    model: &'a Model,
    data: &'a Data,
    vars: Vec<(String, Value)>,
    /// Some aggregate of the current visual row saw rows (the row has data).
    touched: bool,
}

/// One hop along a relationship.
struct Hop {
    from: Id,
    from_col: Id,
    to: Id,
    to_col: Id,
}

pub type Result<T> = std::result::Result<T, String>;

impl<'a> Interp<'a> {
    fn table(&self, id: &str) -> Result<&'a TableData> {
        self.data
            .tables
            .get(id)
            .ok_or_else(|| format!("no data for table {id}"))
    }

    /// Relationships in force: the active ones, with USERELATIONSHIP overrides.
    fn edges(&self, ctx: &Ctx) -> Vec<Hop> {
        let mut out = Vec::new();
        for r in &self.model.relationships {
            let overridden = ctx.use_rels.iter().any(|u| {
                u != &r.id
                    && self.model.relationship(u).is_some_and(|o| {
                        (o.from_table == r.from_table && o.to_table == r.to_table)
                            || (o.from_table == r.to_table && o.to_table == r.from_table)
                    })
            });
            let on = (r.active && !overridden) || ctx.use_rels.contains(&r.id);
            if !on {
                continue;
            }
            let fwd = Hop {
                from: r.from_table.clone(),
                from_col: r.from_column.clone(),
                to: r.to_table.clone(),
                to_col: r.to_column.clone(),
            };
            let back = Hop {
                from: r.to_table.clone(),
                from_col: r.to_column.clone(),
                to: r.from_table.clone(),
                to_col: r.from_column.clone(),
            };
            let both = r.cross_filter == charted_model::CrossFilter::Both;
            match (r.from_cardinality, r.to_cardinality) {
                (Cardinality::Many, Cardinality::One) => {
                    out.push(fwd);
                    if both {
                        out.push(back);
                    }
                }
                (Cardinality::One, Cardinality::Many) => {
                    out.push(back);
                    if both {
                        out.push(fwd);
                    }
                }
                (Cardinality::One, Cardinality::One) => {
                    out.push(fwd);
                    out.push(back);
                }
                _ => {}
            }
        }
        out
    }

    /// The hops from `from` to `to`, if `to` is reachable.
    fn path(&self, ctx: &Ctx, from: &str, to: &str) -> Option<Vec<Hop>> {
        let edges = self.edges(ctx);
        let mut prev: HashMap<Id, usize> = HashMap::new();
        let mut queue = std::collections::VecDeque::from([from.to_string()]);
        let mut seen: HashSet<Id> = HashSet::from([from.to_string()]);
        while let Some(t) = queue.pop_front() {
            if t == to {
                let mut hops = Vec::new();
                let mut cur = t;
                while cur != from {
                    let e = &edges[prev[&cur]];
                    hops.push(Hop {
                        from: e.from.clone(),
                        from_col: e.from_col.clone(),
                        to: e.to.clone(),
                        to_col: e.to_col.clone(),
                    });
                    cur = e.from.clone();
                }
                hops.reverse();
                return Some(hops);
            }
            for (i, e) in edges.iter().enumerate() {
                if e.from == t && seen.insert(e.to.clone()) {
                    prev.insert(e.to.clone(), i);
                    queue.push_back(e.to.clone());
                }
            }
        }
        None
    }

    /// Tables reachable from `t` (its expanded table), including itself.
    fn expanded(&self, ctx: &Ctx, t: &str) -> Vec<Id> {
        self.model
            .tables
            .iter()
            .filter(|x| x.id == t || self.path(ctx, t, &x.id).is_some())
            .map(|x| x.id.clone())
            .collect()
    }

    /// Follows `hops` from a row of the first table; `None` when a related row is missing.
    fn follow(&self, start: &str, row: &[Value], hops: &[Hop]) -> Result<Option<(Id, Vec<Value>)>> {
        let mut table = start.to_string();
        let mut values = row.to_vec();
        for h in hops {
            let from = self.table(&table)?;
            let key = from
                .col(&h.from_col)
                .map(|i| values[i].key())
                .unwrap_or(None);
            let to = self.table(&h.to)?;
            let ci = to.col(&h.to_col).ok_or("bad relationship column")?;
            let Some(key) = key else { return Ok(None) };
            match to
                .rows
                .iter()
                .find(|r| r[ci].key().as_deref() == Some(key.as_str()))
            {
                Some(r) => {
                    values = r.clone();
                    table = h.to.clone();
                }
                None => return Ok(None),
            }
        }
        Ok(Some((table, values)))
    }

    /// Rows of `to` whose `to_col` equals `key`.
    fn related_rows(&self, to: &str, to_col: &str, key: &str) -> Result<Vec<&'a Vec<Value>>> {
        let t = self.table(to)?;
        let ci = t.col(to_col).ok_or("bad relationship column")?;
        Ok(t.rows
            .iter()
            .filter(|r| r[ci].key().as_deref() == Some(key))
            .collect())
    }

    /// A row is visible when some combination of its related rows (following the
    /// relationships left-join style, as SQL does) satisfies every filter that reaches
    /// it. With single-direction relationships there is one combination.
    fn row_visible(&self, ctx: &Ctx, table: &str, row: &[Value]) -> Result<bool> {
        // Breadth-first tree of relationships from `table`.
        let edges = self.edges(ctx);
        let mut order: Vec<usize> = Vec::new();
        let mut reached: Vec<Id> = vec![table.to_string()];
        let mut i = 0;
        while i < reached.len() {
            let t = reached[i].clone();
            for (j, e) in edges.iter().enumerate() {
                if e.from == t && !reached.contains(&e.to) {
                    reached.push(e.to.clone());
                    order.push(j);
                }
            }
            i += 1;
        }
        let filters: Vec<&Filter> = ctx
            .filters
            .iter()
            .filter(|f| reached.contains(&f.table))
            .collect();
        // Only the edges on the way to filtered tables matter.
        let mut needed: Vec<usize> = Vec::new();
        for f in &filters {
            let mut t = f.table.clone();
            while t != table {
                let Some(&j) = order.iter().find(|&&j| edges[j].to == t) else {
                    break;
                };
                if !needed.contains(&j) {
                    needed.push(j);
                }
                t = edges[j].from.clone();
            }
        }
        let mut combos: Vec<HashMap<Id, Option<Vec<Value>>>> =
            vec![HashMap::from([(table.to_string(), Some(row.to_vec()))])];
        for &j in order.iter().filter(|j| needed.contains(j)) {
            let e = &edges[j];
            let mut next = Vec::new();
            for c in combos {
                let key = match c.get(&e.from) {
                    Some(Some(r)) => self
                        .table(&e.from)?
                        .col(&e.from_col)
                        .and_then(|i| r[i].key()),
                    _ => None,
                };
                let related = match key {
                    Some(k) => self.related_rows(&e.to, &e.to_col, &k)?,
                    None => Vec::new(),
                };
                if related.is_empty() {
                    let mut n = c.clone();
                    n.insert(e.to.clone(), None);
                    next.push(n);
                } else {
                    for r in related {
                        let mut n = c.clone();
                        n.insert(e.to.clone(), Some(r.clone()));
                        next.push(n);
                    }
                }
            }
            combos = next;
        }
        for c in combos {
            let mut ok = true;
            for f in &filters {
                let ft = self.table(&f.table)?;
                let target = c.get(&f.table).cloned().flatten();
                let tuple: Tuple = f
                    .columns
                    .iter()
                    .map(|col| match &target {
                        Some(r) => ft.col(col).and_then(|i| r[i].key()),
                        None => None,
                    })
                    .collect();
                if !f.tuples.contains(&tuple) {
                    ok = false;
                    break;
                }
            }
            if ok {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Whether rows of another table refer to keys `table` lacks, which gives `table`
    /// a blank member (DAX's blank row).
    fn has_blank_member(&self, table: &str) -> Result<bool> {
        for r in self
            .model
            .relationships
            .iter()
            .filter(|r| r.active && r.to_table == table)
        {
            let from = self.table(&r.from_table)?;
            let to = self.table(table)?;
            let (Some(fi), Some(ti)) = (from.col(&r.from_column), to.col(&r.to_column)) else {
                continue;
            };
            let keys: HashSet<Option<String>> = to.rows.iter().map(|x| x[ti].key()).collect();
            if from
                .rows
                .iter()
                .any(|x| x[fi].key().is_some() && !keys.contains(&x[fi].key()))
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The blank member's row, when the table has one and (given a context) it is visible.
    fn blank_member(&self, ctx: Option<&Ctx>, table: &str) -> Result<Option<Vec<Value>>> {
        if !self.has_blank_member(table)? {
            return Ok(None);
        }
        let row = vec![Value::Blank; self.table(table)?.columns.len()];
        Ok(match ctx {
            Some(c) if !self.row_visible(c, table, &row)? => None,
            _ => Some(row),
        })
    }

    fn visible_rows(&self, ctx: &Ctx, table: &str) -> Result<Vec<&'a Vec<Value>>> {
        let t = self.table(table)?;
        let mut out = Vec::new();
        for r in &t.rows {
            if self.row_visible(ctx, table, r)? {
                out.push(r);
            }
        }
        Ok(out)
    }

    fn to_row(&self, table: &str, values: &[Value], columns: Option<&[Id]>) -> Result<Row> {
        let t = self.table(table)?;
        let mut map = HashMap::new();
        for (i, c) in t.columns.iter().enumerate() {
            if columns.is_none_or(|cs| cs.contains(c)) {
                map.insert(c.clone(), values[i].clone());
            }
        }
        Ok(Row {
            table: table.to_string(),
            values: map,
        })
    }

    fn distinct(&self, rows: Vec<Row>) -> Vec<Row> {
        let mut seen = HashSet::new();
        rows.into_iter()
            .filter(|r| {
                let mut k: Vec<(Id, Option<String>)> =
                    r.values.iter().map(|(c, v)| (c.clone(), v.key())).collect();
                k.sort();
                seen.insert(k)
            })
            .collect()
    }

    fn eval_table(&mut self, t: &TableExpr, ctx: &Ctx, rows: &[Row]) -> Result<Vec<Row>> {
        match t {
            TableExpr::Table(id) => self
                .visible_rows(ctx, id)?
                .into_iter()
                .map(|r| self.to_row(id, r, None))
                .collect(),
            TableExpr::Values { table, columns } => {
                let mut rs = self
                    .visible_rows(ctx, table)?
                    .into_iter()
                    .map(|r| self.to_row(table, r, Some(columns)))
                    .collect::<Result<Vec<_>>>()?;
                if let Some(b) = self.blank_member(Some(ctx), table)? {
                    rs.push(self.to_row(table, &b, Some(columns))?);
                }
                Ok(self.distinct(rs))
            }
            TableExpr::All { table, columns } => {
                let td = self.table(table)?;
                let mut rs = td
                    .rows
                    .iter()
                    .map(|r| {
                        self.to_row(
                            table,
                            r,
                            if columns.is_empty() {
                                None
                            } else {
                                Some(columns)
                            },
                        )
                    })
                    .collect::<Result<Vec<_>>>()?;
                if let Some(b) = self.blank_member(None, table)? {
                    rs.push(self.to_row(
                        table,
                        &b,
                        if columns.is_empty() {
                            None
                        } else {
                            Some(columns)
                        },
                    )?);
                }
                Ok(if columns.is_empty() {
                    rs
                } else {
                    self.distinct(rs)
                })
            }
            TableExpr::Filter { source, condition } => {
                let src = self.eval_table(source, ctx, rows)?;
                let mut out = Vec::new();
                for r in src {
                    let mut stack = rows.to_vec();
                    stack.push(r.clone());
                    if self.eval(condition, ctx, &stack)?.truthy() {
                        out.push(r);
                    }
                }
                Ok(out)
            }
            TableExpr::List(_) => Err("lists are not tables here".into()),
            TableExpr::Dates { table, column, op } => {
                let td = self.table(table)?;
                let ci = td.col(column).ok_or("missing date column")?;
                let day = |v: &Value| match v {
                    Value::Text(s) => cal::parse(s),
                    _ => None,
                };
                let visible: Vec<i64> = self
                    .visible_rows(ctx, table)?
                    .into_iter()
                    .filter_map(|r| day(&r[ci]))
                    .collect();
                let all: std::collections::BTreeSet<i64> =
                    td.rows.iter().filter_map(|r| day(&r[ci])).collect();
                let (Some(&lo), Some(&hi)) = (visible.iter().min(), visible.iter().max()) else {
                    return Ok(Vec::new());
                };
                let within = |a: i64, b: i64| {
                    if a > b {
                        Vec::new()
                    } else {
                        all.range(a..=b).copied().collect::<Vec<_>>()
                    }
                };
                let picked: Vec<i64> = match *op {
                    DateOp::Shift(n, p) => {
                        let mut out = std::collections::BTreeSet::new();
                        for &v in &visible {
                            let moved = cal::add(v, n, p);
                            if all.contains(&moved) {
                                out.insert(moved);
                            }
                            if p != Period::Day && v == cal::last_day(v) {
                                out.extend(within(moved + 1, cal::last_day(moved)));
                            }
                        }
                        out.into_iter().collect()
                    }
                    DateOp::Parallel(n, p) => within(
                        cal::trunc(cal::add(lo, n, p), p),
                        cal::add(cal::trunc(cal::add(hi, n, p), p), 1, p) - 1,
                    ),
                    DateOp::Previous(p) => {
                        let start = cal::trunc(lo, p);
                        within(cal::add(start, -1, p), start - 1)
                    }
                    DateOp::Next(p) => {
                        let start = cal::trunc(hi, p);
                        within(cal::add(start, 1, p), cal::add(start, 2, p) - 1)
                    }
                    DateOp::ToDate(p, None) => within(cal::trunc(hi, p), hi),
                    DateOp::ToDate(_, Some((m, d))) => {
                        let (y, _, _) = cal::civil(hi);
                        let end = |y: i64| cal::days(y, m, 1) + d as i64 - 1;
                        let start = if hi > end(y) { end(y) } else { end(y - 1) } + 1;
                        within(start, hi)
                    }
                };
                Ok(picked
                    .into_iter()
                    .map(|d| Row {
                        table: table.clone(),
                        values: HashMap::from([(column.clone(), Value::Text(cal::format(d)))]),
                    })
                    .collect())
            }
        }
    }

    /// The filter context with the current rows turned into filters.
    fn transition(&self, ctx: &Ctx, rows: &[Row]) -> Ctx {
        let mut c = ctx.clone();
        for r in rows {
            for (col, v) in &r.values {
                c.filters.retain(|f| !f.columns.contains(col));
                c.filters.push(Filter {
                    table: r.table.clone(),
                    columns: vec![col.clone()],
                    tuples: HashSet::from([vec![v.key()]]),
                    grouping: true,
                });
            }
        }
        c
    }

    fn calculate(
        &mut self,
        expr: &Bound,
        filters: &[FilterArg],
        ctx: &Ctx,
        rows: &[Row],
    ) -> Result<Value> {
        // Filter arguments see the outer context.
        let mut new: Vec<(Filter, bool)> = Vec::new();
        // Time intelligence on a date table replaces the filters on all of its columns.
        let mut date_tables: Vec<Id> = Vec::new();
        for fa in filters {
            let (inner, keep) = match fa {
                FilterArg::Keep(i) => (&**i, true),
                other => (other, false),
            };
            if let FilterArg::Table(t) = inner {
                let table = t.table().ok_or("a list can't filter")?.clone();
                if let TableExpr::Dates { column, .. } = t
                    && !keep
                    && (self.model.table(&table).is_some_and(|x| x.is_date_table)
                        || self
                            .model
                            .relationships
                            .iter()
                            .any(|r| &r.from_column == column || &r.to_column == column))
                {
                    date_tables.push(table.clone());
                }
                let columns: Vec<Id> = match t.columns() {
                    Some(cs) => cs.to_vec(),
                    None => self.table(&table)?.columns.clone(),
                };
                let tuples = self
                    .eval_table(t, ctx, rows)?
                    .into_iter()
                    .map(|r| {
                        columns
                            .iter()
                            .map(|c| r.values.get(c).and_then(Value::key))
                            .collect()
                    })
                    .collect();
                new.push((
                    Filter {
                        table,
                        columns,
                        tuples,
                        grouping: false,
                    },
                    keep,
                ));
            }
        }
        let mut c = self.transition(ctx, rows);
        for fa in filters {
            match fa {
                FilterArg::RemoveFilters { table, columns } => {
                    if columns.is_empty() {
                        let exp = self.expanded(&c, table);
                        c.filters.retain(|f| !exp.contains(&f.table));
                    } else {
                        c.filters
                            .retain(|f| !f.columns.iter().any(|x| columns.contains(x)));
                    }
                }
                FilterArg::AllExcept { table, keep } => {
                    let exp = self.expanded(&c, table);
                    c.filters.retain(|f| {
                        !exp.contains(&f.table) || f.columns.iter().all(|x| keep.contains(x))
                    });
                }
                FilterArg::RemoveGrouping { table, columns } => c.filters.retain(|f| {
                    !(f.grouping
                        && f.table == *table
                        && (columns.is_empty() || f.columns.iter().any(|x| columns.contains(x))))
                }),
                FilterArg::UseRelationship(r) => c.use_rels.push(r.clone()),
                _ => {}
            }
        }
        c.filters.retain(|f| !date_tables.contains(&f.table));
        for (f, keep) in new {
            if !keep {
                c.filters
                    .retain(|x| !x.columns.iter().any(|col| f.columns.contains(col)));
            }
            c.filters.push(f);
        }
        self.eval(expr, &c, rows)
    }

    fn measure(&mut self, id: &str, ctx: &Ctx, rows: &[Row]) -> Result<Value> {
        let m = self
            .model
            .tables
            .iter()
            .flat_map(|t| &t.measures)
            .find(|m| m.id == id)
            .ok_or("missing measure")?;
        let (b, d) = charted_dax::check(self.model, &m.expression);
        let b = b.ok_or_else(|| d.first().map(|d| d.message.clone()).unwrap_or_default())?;
        let saved = std::mem::take(&mut self.vars);
        let v = self.calculate(&b, &[], ctx, rows);
        self.vars = saved;
        v
    }

    fn aggregate(
        &mut self,
        func: AggFunc,
        source: &TableExpr,
        arg: Option<&Bound>,
        ctx: &Ctx,
        rows: &[Row],
    ) -> Result<Value> {
        let src = self.eval_table(source, ctx, rows)?;
        if !src.is_empty() {
            self.touched = true;
        }
        if func == AggFunc::CountRows {
            return Ok(if src.is_empty() {
                Value::Blank
            } else {
                Value::Num(src.len() as f64)
            });
        }
        let mut vals = Vec::with_capacity(src.len());
        for r in src {
            let mut stack = rows.to_vec();
            stack.push(r);
            vals.push(match arg {
                Some(a) => self.eval(a, ctx, &stack)?,
                None => Value::Blank,
            });
        }
        let nonblank: Vec<&Value> = vals.iter().filter(|v| !v.is_blank()).collect();
        Ok(match func {
            AggFunc::Sum => {
                if nonblank.is_empty() {
                    Value::Blank
                } else {
                    Value::Num(nonblank.iter().filter_map(|v| v.num()).sum())
                }
            }
            AggFunc::Average => {
                let ns: Vec<f64> = nonblank.iter().filter_map(|v| v.num()).collect();
                if ns.is_empty() {
                    Value::Blank
                } else {
                    Value::Num(ns.iter().sum::<f64>() / ns.len() as f64)
                }
            }
            AggFunc::Min => nonblank
                .into_iter()
                .min_by(|a, b| cmp(a, b))
                .cloned()
                .unwrap_or(Value::Blank),
            AggFunc::Max => nonblank
                .into_iter()
                .max_by(|a, b| cmp(a, b))
                .cloned()
                .unwrap_or(Value::Blank),
            AggFunc::Count => {
                if nonblank.is_empty() {
                    Value::Blank
                } else {
                    Value::Num(nonblank.len() as f64)
                }
            }
            AggFunc::CountBlank => {
                let n = vals.iter().filter(|v| v.is_blank()).count();
                if n == 0 {
                    Value::Blank
                } else {
                    Value::Num(n as f64)
                }
            }
            AggFunc::DistinctCount => {
                let set: HashSet<Option<String>> = vals.iter().map(Value::key).collect();
                if set.is_empty() {
                    Value::Blank
                } else {
                    Value::Num(set.len() as f64)
                }
            }
            AggFunc::CountRows => unreachable!(),
        })
    }

    fn eval(&mut self, b: &Bound, ctx: &Ctx, rows: &[Row]) -> Result<Value> {
        Ok(match b {
            Bound::Number(n) => Value::Num(*n),
            Bound::Text(s) => Value::Text(s.clone()),
            Bound::Bool(v) => Value::Bool(*v),
            Bound::Blank => Value::Blank,
            Bound::Column { table, column } => rows
                .iter()
                .rev()
                .find(|r| &r.table == table && r.values.contains_key(column))
                .map(|r| r.values[column].clone())
                .unwrap_or(Value::Blank),
            Bound::Related { table, column } => {
                let Some(r) = rows.last() else {
                    return Ok(Value::Blank);
                };
                if &r.table == table {
                    return Ok(r.values.get(column).cloned().unwrap_or(Value::Blank));
                }
                let td = self.table(&r.table)?;
                let full: Vec<Value> = td
                    .columns
                    .iter()
                    .map(|c| r.values.get(c).cloned().unwrap_or(Value::Blank))
                    .collect();
                let Some(hops) = self.path(ctx, &r.table, table) else {
                    return Ok(Value::Blank);
                };
                match self.follow(&r.table, &full, &hops)? {
                    Some((t, vals)) => self
                        .table(&t)?
                        .col(column)
                        .map(|i| vals[i].clone())
                        .unwrap_or(Value::Blank),
                    None => Value::Blank,
                }
            }
            Bound::Measure { measure, .. } => self.measure(measure, ctx, rows)?,
            Bound::Var(name) => self
                .vars
                .iter()
                .rev()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
                .ok_or_else(|| format!("unknown variable {name}"))?,
            Bound::Let { name, value, body } => {
                let v = self.eval(value, ctx, rows)?;
                self.vars.push((name.clone(), v));
                let out = self.eval(body, ctx, rows);
                self.vars.pop();
                out?
            }
            Bound::Aggregate { func, source, arg } => {
                self.aggregate(*func, source, arg.as_deref(), ctx, rows)?
            }
            Bound::Calculate { expr, filters } => self.calculate(expr, filters, ctx, rows)?,
            Bound::Call { func, args } => {
                let a = args
                    .iter()
                    .map(|x| self.eval(x, ctx, rows))
                    .collect::<Result<Vec<_>>>()?;
                match func {
                    ScalarFunc::Divide => match (a[0].num(), a[1].num()) {
                        (_, None) | (_, Some(0.0)) => a.get(2).cloned().unwrap_or(Value::Blank),
                        (None, _) => Value::Blank,
                        (Some(x), Some(y)) => Value::Num(x / y),
                    },
                    ScalarFunc::If => {
                        if a[0].truthy() {
                            a[1].clone()
                        } else {
                            a.get(2).cloned().unwrap_or(Value::Blank)
                        }
                    }
                    ScalarFunc::Abs => a[0]
                        .num()
                        .map(|x| Value::Num(x.abs()))
                        .unwrap_or(Value::Blank),
                    ScalarFunc::Round => match (a[0].num(), a[1].num()) {
                        (Some(x), Some(d)) => {
                            let m = 10f64.powi(d as i32);
                            Value::Num((x * m).round() / m)
                        }
                        _ => Value::Blank,
                    },
                    ScalarFunc::Coalesce => a
                        .into_iter()
                        .find(|v| !v.is_blank())
                        .unwrap_or(Value::Blank),
                    ScalarFunc::And => Value::Bool(a[0].truthy() && a[1].truthy()),
                    ScalarFunc::Or => Value::Bool(a[0].truthy() || a[1].truthy()),
                    ScalarFunc::Not => Value::Bool(!a[0].truthy()),
                    ScalarFunc::IsBlank => Value::Bool(a[0].is_blank()),
                }
            }
            Bound::Info {
                func,
                table,
                column,
                alternate,
            } => {
                match func {
                    InfoFunc::SelectedValue | InfoFunc::HasOneValue => {
                        let col = column.clone().unwrap_or_default();
                        let vals = self.eval_table(
                            &TableExpr::Values {
                                table: table.clone(),
                                columns: vec![col.clone()],
                            },
                            ctx,
                            rows,
                        )?;
                        if !vals.is_empty() {
                            self.touched = true;
                        }
                        let one = vals.len() == 1;
                        if *func == InfoFunc::HasOneValue {
                            Value::Bool(one)
                        } else if one {
                            vals[0].values.get(&col).cloned().unwrap_or(Value::Blank)
                        } else {
                            match alternate {
                                Some(a) => self.eval(a, ctx, rows)?,
                                None => Value::Blank,
                            }
                        }
                    }
                    InfoFunc::IsFiltered => Value::Bool(ctx.filters.iter().any(|f| {
                        &f.table == table && column.as_ref().is_none_or(|c| f.columns.contains(c))
                    })),
                    InfoFunc::IsInScope => Value::Bool(ctx.filters.iter().any(|f| {
                        f.grouping && column.as_ref().is_some_and(|c| f.columns.contains(c))
                    })),
                }
            }
            Bound::Switch {
                value,
                cases,
                default,
            } => {
                // Every branch is evaluated, as the compiler computes them all.
                let v = self.eval(value, ctx, rows)?;
                let mut hit: Option<Value> = None;
                for (w, then) in cases {
                    let wv = self.eval(w, ctx, rows)?;
                    let tv = self.eval(then, ctx, rows)?;
                    let matched = match &v {
                        Value::Bool(true) => wv.truthy(),
                        _ => cmp(&coerced(&v, &wv), &coerced(&wv, &v)).is_eq(),
                    };
                    if hit.is_none() && matched {
                        hit = Some(tv);
                    }
                }
                let d = match default {
                    Some(d) => self.eval(d, ctx, rows)?,
                    None => Value::Blank,
                };
                hit.unwrap_or(d)
            }
            Bound::In { value, list } => {
                let v = self.eval(value, ctx, rows)?;
                let mut hit = false;
                for item in list {
                    let iv = self.eval(item, ctx, rows)?;
                    if cmp(&coerced(&v, &iv), &coerced(&iv, &v)).is_eq() {
                        hit = true;
                    }
                }
                Value::Bool(hit)
            }
            Bound::Unary { op, expr } => {
                let v = self.eval(expr, ctx, rows)?;
                match op {
                    UnaryOp::Neg => v.num().map(|x| Value::Num(-x)).unwrap_or(Value::Blank),
                    UnaryOp::Plus => v,
                    UnaryOp::Not => Value::Bool(!v.truthy()),
                }
            }
            Bound::Binary { op, left, right } => {
                let l = self.eval(left, ctx, rows)?;
                let r = self.eval(right, ctx, rows)?;
                binary(*op, &l, &r)
            }
        })
    }
}

fn binary(op: BinaryOp, l: &Value, r: &Value) -> Value {
    let n = |v: &Value| v.num().unwrap_or(0.0);
    match op {
        BinaryOp::Add | BinaryOp::Sub => {
            if l.is_blank() && r.is_blank() {
                Value::Blank
            } else if op == BinaryOp::Add {
                Value::Num(n(l) + n(r))
            } else {
                Value::Num(n(l) - n(r))
            }
        }
        BinaryOp::Mul => {
            if l.is_blank() || r.is_blank() {
                Value::Blank
            } else {
                Value::Num(n(l) * n(r))
            }
        }
        BinaryOp::Div => {
            if l.is_blank() || r.is_blank() || n(r) == 0.0 {
                Value::Blank
            } else {
                Value::Num(n(l) / n(r))
            }
        }
        BinaryOp::Pow => {
            if l.is_blank() || r.is_blank() {
                Value::Blank
            } else {
                Value::Num(n(l).powf(n(r)))
            }
        }
        BinaryOp::Concat => Value::Text(format!(
            "{}{}",
            l.key().unwrap_or_default(),
            r.key().unwrap_or_default()
        )),
        BinaryOp::StrictEq => Value::Bool(l == r),
        BinaryOp::And => Value::Bool(l.truthy() && r.truthy()),
        BinaryOp::Or => Value::Bool(l.truthy() || r.truthy()),
        BinaryOp::In => Value::Blank,
        cmpop => {
            let o = cmp(&coerced(l, r), &coerced(r, l));
            Value::Bool(match cmpop {
                BinaryOp::Eq => o.is_eq(),
                BinaryOp::Ne => !o.is_eq(),
                BinaryOp::Lt => o.is_lt(),
                BinaryOp::Le => o.is_le(),
                BinaryOp::Gt => o.is_gt(),
                _ => o.is_ge(),
            })
        }
    }
}

/// The expression behind a projection: a measure, or a summarised column.
fn projection_bound(model: &Model, p: &Projection) -> Result<Bound> {
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
        (FieldRef::Column { .. }, None) => {
            let _ = model;
            Err("a column used as a value needs an aggregation".into())
        }
    }
}

fn condition_holds(v: &Value, c: &Condition, text: bool) -> bool {
    let s = v.key().unwrap_or_default().to_lowercase();
    let lit = c.value.clone().unwrap_or_default();
    let litv = if text {
        Value::Text(lit.clone())
    } else {
        lit.trim()
            .parse::<f64>()
            .map(Value::Num)
            .unwrap_or(Value::Text(lit.clone()))
    };
    let o = || cmp(v, &litv);
    match c.op {
        FilterOp::IsBlank => v.is_blank() || s.is_empty(),
        FilterOp::IsNotBlank => !v.is_blank() && !s.is_empty(),
        _ if v.is_blank() => matches!(c.op, FilterOp::NotEquals | FilterOp::NotContains),
        FilterOp::Contains => s.contains(&lit.to_lowercase()),
        FilterOp::NotContains => !s.contains(&lit.to_lowercase()),
        FilterOp::StartsWith => s.starts_with(&lit.to_lowercase()),
        FilterOp::EndsWith => s.ends_with(&lit.to_lowercase()),
        FilterOp::Equals if text => s == lit.to_lowercase(),
        FilterOp::NotEquals if text => s != lit.to_lowercase(),
        FilterOp::Equals => o().is_eq(),
        FilterOp::NotEquals => !o().is_eq(),
        FilterOp::Less => o().is_lt(),
        FilterOp::LessOrEqual => o().is_le(),
        FilterOp::Greater => o().is_gt(),
        FilterOp::GreaterOrEqual => o().is_ge(),
        FilterOp::Between => {
            let hi = c.value2.clone().unwrap_or_default();
            let hiv = hi
                .trim()
                .parse::<f64>()
                .map(Value::Num)
                .unwrap_or(Value::Text(hi));
            o().is_ge() && cmp(v, &hiv).is_le()
        }
    }
}

fn kind_holds(v: &Value, kind: &FilterKind, text: bool) -> bool {
    match kind {
        // The interpreter has no clock; relative dates are tested against SQL directly.
        FilterKind::All | FilterKind::TopN { .. } | FilterKind::RelativeDate { .. } => true,
        FilterKind::Basic { values, exclude } => values.contains(&v.key()) != *exclude,
        FilterKind::Advanced { conditions, or } => {
            if *or {
                conditions.iter().any(|c| condition_holds(v, c, text))
            } else {
                conditions.iter().all(|c| condition_holds(v, c, text))
            }
        }
    }
}

/// Evaluates a visual query. Rows hold the groups then the values; they come back in
/// the order requested (by default, ascending by the groups).
pub fn run(model: &Model, data: &Data, q: &VisualQuery) -> Result<Vec<Vec<Value>>> {
    let mut it = Interp {
        model,
        data,
        vars: Vec::new(),
        touched: false,
    };
    let mut groups: Vec<(Id, Id)> = Vec::new();
    for p in &q.groups {
        match &p.field {
            FieldRef::Column { table, column } => groups.push((table.clone(), column.clone())),
            _ => return Err("only columns can be grouped by".into()),
        }
    }
    let values = q
        .values
        .iter()
        .map(|p| projection_bound(model, p))
        .collect::<Result<Vec<_>>>()?;

    // Column filters become allowed-value filters; Top N filters are evaluated with the
    // others in place; measure filters apply to the finished rows.
    let mut base = Ctx::default();
    let mut topn: Vec<(usize, &QueryFilterRef)> = Vec::new();
    let refs: Vec<QueryFilterRef> = q
        .filters
        .iter()
        .map(|f| QueryFilterRef {
            field: &f.field,
            kind: &f.filter,
        })
        .collect();
    for (i, f) in refs.iter().enumerate() {
        if let (FieldRef::Column { table, column }, None) = (&f.field.field, f.field.aggregation) {
            {
                if matches!(f.kind, FilterKind::TopN { .. }) {
                    topn.push((i, f));
                    continue;
                }
                let td = it.table(table)?;
                let ci = td.col(column).ok_or("bad filter column")?;
                let text = model
                    .table(table)
                    .and_then(|t| t.column(column))
                    .is_some_and(|c| c.data_type == charted_model::DataType::String);
                let mut tuples = HashSet::new();
                for r in &td.rows {
                    if kind_holds(&r[ci], f.kind, text) {
                        tuples.insert(vec![r[ci].key()]);
                    }
                }
                base.filters.push(Filter {
                    table: table.clone(),
                    columns: vec![column.clone()],
                    tuples,
                    grouping: false,
                });
            }
        }
    }
    for (i, f) in topn {
        let FilterKind::TopN { count, bottom, by } = f.kind else {
            continue;
        };
        let others: Vec<_> = q
            .filters
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, f)| f.clone())
            .collect();
        let sub = run(
            model,
            data,
            &VisualQuery {
                groups: vec![f.field.clone()],
                values: vec![by.clone()],
                filters: others,
                order: vec![crate::visual::OrderBy {
                    target: OrderTarget::Value(0),
                    descending: !bottom,
                }],
                limit: Some(*count),
                keys: false,
            },
        )?;
        let FieldRef::Column { table, column } = &f.field.field else {
            continue;
        };
        base.filters.push(Filter {
            table: table.clone(),
            columns: vec![column.clone()],
            tuples: sub.iter().map(|r| vec![r[0].key()]).collect(),
            grouping: false,
        });
    }

    // Candidate rows: per table, the distinct visible tuples of its grouping columns.
    let mut by_table: Vec<(Id, Vec<usize>)> = Vec::new();
    for (i, (t, _)) in groups.iter().enumerate() {
        match by_table.iter_mut().find(|(x, _)| x == t) {
            Some((_, idx)) => idx.push(i),
            None => by_table.push((t.clone(), vec![i])),
        }
    }
    let mut combos: Vec<Vec<Value>> = vec![vec![Value::Blank; groups.len()]];
    for (t, idx) in &by_table {
        let td = it.table(t)?;
        let mut seen = HashSet::new();
        let mut tuples = Vec::new();
        let blank_row = it.blank_member(Some(&base), t)?;
        let mut rows = it.visible_rows(&base, t)?;
        // The blank member, when another table refers to keys this one lacks.
        if let Some(b) = &blank_row {
            rows.push(b);
        }
        for r in rows {
            let tuple: Vec<Value> = idx
                .iter()
                .map(|&i| {
                    td.col(&groups[i].1)
                        .map(|c| r[c].clone())
                        .unwrap_or(Value::Blank)
                })
                .collect();
            if seen.insert(tuple.iter().map(Value::key).collect::<Vec<_>>()) {
                tuples.push(tuple);
            }
        }
        let mut next = Vec::new();
        for c in &combos {
            for tu in &tuples {
                let mut n = c.clone();
                for (k, &i) in idx.iter().enumerate() {
                    n[i] = tu[k].clone();
                }
                next.push(n);
            }
        }
        combos = next;
    }

    // Values that never read rows (constants, ISINSCOPE…) show on every row.
    let needs_data = values.iter().any(|v| reads_rows(model, v, &mut Vec::new()));
    let mut out = Vec::new();
    for combo in combos {
        let mut ctx = base.clone();
        for (i, (t, c)) in groups.iter().enumerate() {
            ctx.filters.push(Filter {
                table: t.clone(),
                columns: vec![c.clone()],
                tuples: HashSet::from([vec![combo[i].key()]]),
                grouping: true,
            });
        }
        it.touched = false;
        let vals = values
            .iter()
            .map(|v| it.eval(v, &ctx, &[]))
            .collect::<Result<Vec<_>>>()?;
        // Rows where no aggregate saw data, or every value is blank, are left out.
        if !values.is_empty()
            && !groups.is_empty()
            && ((!it.touched && needs_data) || vals.iter().all(Value::is_blank))
        {
            continue;
        }
        // Measure filters.
        let mut keep = true;
        for f in &refs {
            if matches!(f.field.field, FieldRef::Column { .. }) && f.field.aggregation.is_none() {
                continue;
            }
            let v = it.eval(&projection_bound(model, f.field)?, &ctx, &[])?;
            keep &= kind_holds(&v, f.kind, false);
        }
        if keep {
            let mut row = combo;
            row.extend(vals);
            out.push(row);
        }
    }

    // Order: requested keys, then every group ascending (blank first).
    let n = groups.len();
    out.sort_by(|a, b| {
        for o in &q.order {
            let i = match o.target {
                OrderTarget::Group(i) => i,
                OrderTarget::Value(i) => n + i,
            };
            let c = cmp(&a[i], &b[i]);
            let c = if o.descending { c.reverse() } else { c };
            if !c.is_eq() {
                return c;
            }
        }
        for i in 0..n {
            let c = cmp(&a[i], &b[i]);
            if !c.is_eq() {
                return c;
            }
        }
        std::cmp::Ordering::Equal
    });
    if let Some(l) = q.limit {
        out.truncate(l as usize);
    }
    Ok(out)
}

/// Whether evaluating an expression reads table rows (aggregates, VALUES…).
fn reads_rows(model: &Model, b: &Bound, stack: &mut Vec<Id>) -> bool {
    let mut found = false;
    charted_dax::bind::walk(b, &mut |x| match x {
        Bound::Aggregate { .. } => found = true,
        Bound::Info {
            func: InfoFunc::SelectedValue | InfoFunc::HasOneValue,
            ..
        } => found = true,
        Bound::Measure { measure, .. } if !stack.contains(measure) => {
            stack.push(measure.clone());
            if let Some(m) = model
                .tables
                .iter()
                .flat_map(|t| &t.measures)
                .find(|m| &m.id == measure)
                && let (Some(inner), _) = charted_dax::check(model, &m.expression)
            {
                found |= reads_rows(model, &inner, stack);
            }
        }
        _ => {}
    });
    found
}

struct QueryFilterRef<'a> {
    field: &'a Projection,
    kind: &'a FilterKind,
}

/// Calendar arithmetic on days since 1970-01-01 (what DuckDB's date functions do).
mod cal {
    use charted_dax::Period;

    pub fn days(y: i64, m: u32, d: u32) -> i64 {
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let mp = (m as i64 + 9) % 12;
        let doy = (153 * mp + 2) / 5 + d as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    pub fn civil(z: i64) -> (i64, u32, u32) {
        let z = z + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        (yoe + era * 400 + i64::from(m <= 2), m, d)
    }

    pub fn parse(s: &str) -> Option<i64> {
        let mut it = s.get(..10)?.split('-');
        let y = it.next()?.parse().ok()?;
        let m = it.next()?.parse().ok()?;
        let d = it.next()?.parse().ok()?;
        Some(days(y, m, d))
    }

    pub fn format(z: i64) -> String {
        let (y, m, d) = civil(z);
        format!("{y:04}-{m:02}-{d:02}")
    }

    pub fn last_day(z: i64) -> i64 {
        let (y, m, _) = civil(z);
        let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
        days(ny, nm, 1) - 1
    }

    /// Adds `n` periods; a day past the end of the target month becomes its last day.
    pub fn add(z: i64, n: i32, p: Period) -> i64 {
        let months = match p {
            Period::Day => return z + n as i64,
            Period::Month => n as i64,
            Period::Quarter => 3 * n as i64,
            Period::Year => 12 * n as i64,
        };
        let (y, m, d) = civil(z);
        let total = y * 12 + (m as i64 - 1) + months;
        let (ny, nm) = (total.div_euclid(12), (total.rem_euclid(12) + 1) as u32);
        let first = days(ny, nm, 1);
        (first + d as i64 - 1).min(last_day(first))
    }

    /// The first day of the period holding `z`.
    pub fn trunc(z: i64, p: Period) -> i64 {
        let (y, m, _) = civil(z);
        match p {
            Period::Day => z,
            Period::Month => days(y, m, 1),
            Period::Quarter => days(y, (m - 1) / 3 * 3 + 1, 1),
            Period::Year => days(y, 1, 1),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn round_trips_and_clamps() {
            for s in ["1970-01-01", "2024-02-29", "2025-12-31", "1999-03-01"] {
                assert_eq!(format(parse(s).unwrap()), s);
            }
            assert_eq!(
                format(add(parse("2024-01-31").unwrap(), 1, Period::Month)),
                "2024-02-29"
            );
            assert_eq!(
                format(add(parse("2024-02-29").unwrap(), -1, Period::Year)),
                "2023-02-28"
            );
            assert_eq!(
                format(trunc(parse("2025-08-17").unwrap(), Period::Quarter)),
                "2025-07-01"
            );
            assert_eq!(format(last_day(parse("2023-02-03").unwrap())), "2023-02-28");
        }
    }
}
