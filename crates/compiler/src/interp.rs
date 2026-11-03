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
    AggFunc, Anchor, ArgKind, BinaryOp, Bound, DateOp, DatePart, FilterArg, FormatSpec, InfoFunc,
    LibFn, Period, ScalarFunc, TableExpr, UnaryOp,
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

    /// The value as a number, dates as their serial numbers (what arithmetic uses).
    fn numeric(&self) -> Option<f64> {
        self.num().or_else(|| self.serial())
    }

    /// A date or date/time (ISO text here, as DuckDB writes them) as DAX's serial
    /// number: days since 1899-12-30, the time a fraction of a day.
    fn serial(&self) -> Option<f64> {
        match self {
            Value::Text(t) => cal::serial(t),
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
        _ => match (a.numeric(), b.numeric()) {
            // NaN after every number, as DuckDB orders it.
            (Some(x), Some(y)) => match (x.is_nan(), y.is_nan()) {
                (false, false) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
                (a, b) => a.cmp(&b),
            },
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
    /// Which variables hold dates (by their expression's type), as `vars`.
    dated_vars: Vec<(String, bool)>,
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
            TableExpr::AllSelected { table, columns } => {
                let mut c = ctx.clone();
                c.filters.retain(|f| !f.grouping);
                // The whole table is its distinct rows, with the blank row (as in Power BI).
                let t = if columns.is_empty() {
                    TableExpr::Values {
                        table: table.clone(),
                        columns: self.table(table)?.columns.clone(),
                    }
                } else {
                    TableExpr::Values {
                        table: table.clone(),
                        columns: columns.clone(),
                    }
                };
                self.eval_table(&t, &c, rows)
            }
            TableExpr::Filter { source, condition } => {
                let src = self.eval_table(source, ctx, rows)?;
                // Rows read to test the condition (other colours' sales in FILTER ( VALUES
                // ( Product[Color] ), [Sales] > 0 )) don't give the visual row data.
                let touched = self.touched;
                let mut out = Vec::new();
                for r in src {
                    let mut stack = rows.to_vec();
                    stack.push(r.clone());
                    if self.eval(condition, ctx, &stack)?.truthy() {
                        out.push(r);
                    }
                }
                self.touched = touched;
                Ok(out)
            }
            TableExpr::List(list) => {
                let names = t.ext_names();
                let mut out = Vec::new();
                for r in list {
                    let mut values = HashMap::new();
                    for (n, b) in names.iter().zip(r) {
                        values.insert(charted_dax::bind::ext_key(n), self.eval(b, ctx, rows)?);
                    }
                    out.push(Row {
                        table: String::new(),
                        values,
                    });
                }
                Ok(out)
            }
            TableExpr::CalculateTable { source, filters } => {
                let c = self.calc_ctx(filters, ctx, rows)?;
                self.eval_table(source, &c, rows)
            }
            TableExpr::AddColumns { source, cols } | TableExpr::SelectColumns { source, cols } => {
                let select = matches!(t, TableExpr::SelectColumns { .. });
                let mut out = Vec::new();
                for r in self.eval_table(source, ctx, rows)? {
                    let mut stack = rows.to_vec();
                    stack.push(r.clone());
                    let mut values = if select {
                        HashMap::new()
                    } else {
                        r.values.clone()
                    };
                    for (name, x) in cols {
                        let v = self.eval(x, ctx, &stack)?;
                        // A plain column keeps its lineage.
                        if let (true, Bound::Column { column, .. }) = (select, x) {
                            values.insert(column.clone(), v.clone());
                        }
                        values.insert(charted_dax::bind::ext_key(name), v);
                    }
                    out.push(Row {
                        table: r.table.clone(),
                        values,
                    });
                }
                Ok(out)
            }
            TableExpr::Summarize { source, group } => {
                let mut out = Vec::new();
                for r in self.eval_table(source, ctx, rows)? {
                    let mut values = HashMap::new();
                    for (gt, gc) in group {
                        let v = match r.values.get(gc) {
                            Some(v) => v.clone(),
                            None => {
                                let mut stack = rows.to_vec();
                                stack.push(r.clone());
                                let rel = Bound::Related {
                                    table: gt.clone(),
                                    column: gc.clone(),
                                };
                                self.eval(&rel, ctx, &stack)?
                            }
                        };
                        values.insert(gc.clone(), v);
                    }
                    out.push(Row {
                        table: r.table.clone(),
                        values,
                    });
                }
                Ok(self.distinct(out))
            }
            TableExpr::SummarizeColumns {
                group,
                filters,
                cols,
            } => {
                let c = self.calc_ctx(filters, ctx, rows)?;
                // Each table's visible combinations of its group columns, crossed.
                let mut tables: Vec<Id> = Vec::new();
                for (gt, _) in group {
                    if !tables.contains(gt) {
                        tables.push(gt.clone());
                    }
                }
                let mut combos: Vec<HashMap<Id, Value>> = vec![HashMap::new()];
                for gt in &tables {
                    let columns: Vec<Id> = group
                        .iter()
                        .filter(|(t, _)| t == gt)
                        .map(|(_, c)| c.clone())
                        .collect();
                    let vals = self.eval_table(
                        &TableExpr::Values {
                            table: gt.clone(),
                            columns,
                        },
                        &c,
                        rows,
                    )?;
                    let mut next = Vec::new();
                    for base in &combos {
                        for v in &vals {
                            let mut m = base.clone();
                            m.extend(v.values.clone());
                            next.push(m);
                        }
                    }
                    combos = next;
                }
                let mut out = Vec::new();
                for values in combos {
                    let mut row = Row {
                        table: tables[0].clone(),
                        values,
                    };
                    if !cols.is_empty() {
                        let mut stack = rows.to_vec();
                        stack.push(row.clone());
                        let mut any = false;
                        let mut added = Vec::new();
                        for (name, x) in cols {
                            let v = self.eval(x, &c, &stack)?;
                            any |= !v.is_blank();
                            added.push((charted_dax::bind::ext_key(name), v));
                        }
                        if !any {
                            continue;
                        }
                        row.values.extend(added);
                    }
                    out.push(row);
                }
                Ok(out)
            }
            TableExpr::Calendar { start, end } => {
                let day = |v: &Value| match v {
                    Value::Text(s) => cal::parse(s),
                    _ => None,
                };
                let (a, b) = (self.eval(start, ctx, rows)?, self.eval(end, ctx, rows)?);
                let (Some(a), Some(b)) = (day(&a), day(&b)) else {
                    return Ok(Vec::new());
                };
                Ok((a..=b)
                    .map(|d| Row {
                        table: String::new(),
                        values: HashMap::from([(
                            charted_dax::bind::ext_key("Date"),
                            Value::Text(cal::format(d)),
                        )]),
                    })
                    .collect())
            }
            TableExpr::GenerateSeries { start, end, step } => {
                let num = |v: Value| v.num().unwrap_or(0.0);
                let (a, b, s) = (
                    num(self.eval(start, ctx, rows)?),
                    num(self.eval(end, ctx, rows)?),
                    num(self.eval(step, ctx, rows)?),
                );
                if s <= 0.0 {
                    return Err("GENERATESERIES needs a positive increment".into());
                }
                let n = ((b - a) / s + 1e-9).floor();
                Ok((0..=(n.max(-1.0) as i64))
                    .filter(|_| n >= 0.0)
                    .map(|i| Row {
                        table: String::new(),
                        values: HashMap::from([(
                            charted_dax::bind::ext_key("Value"),
                            Value::Num(a + i as f64 * s),
                        )]),
                    })
                    .collect())
            }
            TableExpr::TopN { n, source, order } => {
                let n = self.eval(n, ctx, rows)?.num().unwrap_or(0.0);
                let mut keyed = Vec::new();
                for r in self.eval_table(source, ctx, rows)? {
                    let mut stack = rows.to_vec();
                    stack.push(r.clone());
                    let keys = order
                        .iter()
                        .map(|(o, _)| self.eval(o, ctx, &stack))
                        .collect::<Result<Vec<_>>>()?;
                    keyed.push((r, keys));
                }
                // Blank orders as zero (or empty text); ties at the cut are all kept.
                let likes: Vec<Value> = (0..order.len())
                    .map(|i| {
                        keyed
                            .iter()
                            .map(|(_, k)| &k[i])
                            .find(|v| !v.is_blank())
                            .cloned()
                            .unwrap_or(Value::Num(0.0))
                    })
                    .collect();
                let before = |a: &[Value], b: &[Value]| {
                    for (i, (_, asc)) in order.iter().enumerate() {
                        let o = cmp(&coerced(&a[i], &likes[i]), &coerced(&b[i], &likes[i]));
                        let o = if *asc { o } else { o.reverse() };
                        if !o.is_eq() {
                            return o.is_lt();
                        }
                    }
                    false
                };
                Ok(keyed
                    .iter()
                    .filter(|(_, k)| {
                        let rank = 1 + keyed.iter().filter(|(_, o)| before(o, k)).count();
                        (rank as f64) <= n
                    })
                    .map(|(r, _)| r.clone())
                    .collect())
            }
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
                // Ends given as expressions are evaluated in the outer context; only ends
                // that are the first or last visible date need visible dates.
                let anchors: Vec<&Anchor> = match op {
                    DateOp::InPeriod(a, ..) => vec![a],
                    DateOp::Between(a, b) => vec![a, b],
                    _ => Vec::new(),
                };
                let mut ends: Vec<Option<i64>> = Vec::new();
                for a in &anchors {
                    if let Anchor::Expr(e) = a {
                        let v = self.eval(e, ctx, rows)?;
                        ends.push(day(&v));
                    }
                }
                let needs_visible = anchors.is_empty()
                    || anchors
                        .iter()
                        .any(|a| matches!(a, Anchor::First | Anchor::Last));
                let (lo, hi) = match (visible.iter().min(), visible.iter().max()) {
                    (Some(&lo), Some(&hi)) => (lo, hi),
                    _ if needs_visible => return Ok(Vec::new()),
                    _ => (0, 0),
                };
                let next_end = std::cell::Cell::new(0);
                // A blank end falls back to `open`, as BLANK () does.
                let anchor = |a: &Anchor, open: i64| match a {
                    Anchor::First => lo,
                    Anchor::Last => hi,
                    Anchor::Date(y, m, d) => cal::date(*y as i64, *m as i64, *d as i64),
                    Anchor::Open => open,
                    Anchor::Expr(_) => {
                        let i = next_end.get();
                        next_end.set(i + 1);
                        ends[i].unwrap_or(open)
                    }
                };
                let within = |a: i64, b: i64| {
                    if a > b {
                        Vec::new()
                    } else {
                        all.range(a..=b).copied().collect::<Vec<_>>()
                    }
                };
                let picked: Vec<i64> = match op.clone() {
                    // The visible range moves: its first date by `n` units (the day
                    // clamped), its last too, or to the target month's last day when the
                    // range ends on a month's last day and has more than one date. A
                    // selection with gaps gives no dates (Power BI's error, which the SQL
                    // can't raise only where the value is used).
                    DateOp::Shift(n, p) => {
                        if all.range(lo..=hi).count() > visible.iter().collect::<HashSet<_>>().len()
                        {
                            return Ok(Vec::new());
                        }
                        let moved = cal::add(hi, n, p);
                        let end = if p != Period::Day && lo < hi && hi == cal::last_day(hi) {
                            cal::last_day(moved)
                        } else {
                            moved
                        };
                        within(cal::add(lo, n, p), end)
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
                    DateOp::InPeriod(a, n, p) => {
                        // An anchor before the column's first date starts there.
                        let at = anchor(&a, lo).max(all.iter().next().copied().unwrap_or(i64::MIN));
                        if n < 0 {
                            // Back from a month's last day to the target month's last day.
                            let back = if p != Period::Day && at == cal::last_day(at) {
                                cal::last_day(cal::add(at, n, p))
                            } else {
                                cal::add(at, n, p)
                            };
                            within(back + 1, at)
                        } else {
                            within(at, cal::add(at, n, p) - 1)
                        }
                    }
                    DateOp::Between(a, b) => within(anchor(&a, i64::MIN), anchor(&b, i64::MAX)),
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
            // A row of a date table (its date column) replaces every filter on the
            // table, as a CALCULATE filter on the date column does.
            let cols: Vec<Id> = r
                .values
                .keys()
                .filter(|c| !c.starts_with('\u{0}'))
                .cloned()
                .collect();
            if crate::replaces_date_table(self.model, &r.table, &cols) {
                c.filters.retain(|f| f.table != r.table);
            }
            for (col, v) in &r.values {
                // Added columns don't filter; carried columns filter their own table.
                if col.starts_with('\u{0}') {
                    continue;
                }
                let table = self
                    .model
                    .tables
                    .iter()
                    .find(|t| t.column(col).is_some())
                    .map(|t| t.id.clone())
                    .unwrap_or_else(|| r.table.clone());
                c.filters.retain(|f| !f.columns.contains(col));
                c.filters.push(Filter {
                    table,
                    columns: vec![col.clone()],
                    tuples: HashSet::from([vec![v.key()]]),
                    grouping: true,
                });
            }
            // The row filters its expanded table: each table it reaches on the one side
            // of an active relationship is filtered to the related row, replacing the
            // filters on that table's columns (as in Power BI).
            for rel in self
                .model
                .relationships
                .iter()
                .filter(|rel| rel.active && rel.from_table == r.table)
            {
                let Some(v) = r.values.get(&rel.from_column) else {
                    continue;
                };
                let one = &rel.to_table;
                // A key the table lacks relates the row to its blank member.
                let matched = self
                    .table(one)
                    .ok()
                    .and_then(|t| t.col(&rel.to_column).map(|i| (t, i)))
                    .is_some_and(|(t, i)| t.rows.iter().any(|x| x[i].key() == v.key()));
                let key = if matched { v.key() } else { None };
                let cols: Vec<&Id> = self
                    .model
                    .table(one)
                    .map(|t| t.columns.iter().map(|c| &c.id).collect())
                    .unwrap_or_default();
                c.filters.retain(|f| {
                    &f.table != one && !f.columns.iter().any(|col| cols.contains(&col))
                });
                c.filters.push(Filter {
                    table: one.clone(),
                    columns: vec![rel.to_column.clone()],
                    tuples: HashSet::from([vec![key]]),
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
        let c = self.calc_ctx(filters, ctx, rows)?;
        self.eval(expr, &c, rows)
    }

    /// The context CALCULATE's filters make of `ctx` (with the current rows transitioned).
    fn calc_ctx(&mut self, filters: &[FilterArg], ctx: &Ctx, rows: &[Row]) -> Result<Ctx> {
        // Filter arguments see the outer context. Rows they read (other regions' sales
        // in `FILTER ( ALL ( Store[Region] ), [Sales] > 0 )`) don't give the visual row
        // data.
        let touched = self.touched;
        let mut new: Vec<(Filter, bool)> = Vec::new();
        // Filters on a date table's date column replace the filters on all of its columns.
        let mut date_tables: Vec<Id> = Vec::new();
        for fa in filters {
            let (inner, keep) = match fa {
                FilterArg::Keep(i) => (&**i, true),
                other => (other, false),
            };
            if let FilterArg::Table(t) = inner {
                let table = t.table().ok_or("a list can't filter")?.clone();
                let columns: Vec<Id> = match t.columns() {
                    Some(cs) => cs.to_vec(),
                    None => self.table(&table)?.columns.clone(),
                };
                // A filter on a date table's date column replaces all its filters.
                if !keep && crate::replaces_date_table(self.model, &table, &columns) {
                    date_tables.push(table.clone());
                }
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
        self.touched = touched;
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
                FilterArg::RemoveAll => c.filters.clear(),
                _ => {}
            }
        }
        c.filters.retain(|f| !date_tables.contains(&f.table));
        // Filter arguments replace the filters already there, and are ANDed together.
        for (f, keep) in &new {
            if !keep {
                c.filters
                    .retain(|x| !x.columns.iter().any(|col| f.columns.contains(col)));
            }
        }
        c.filters.extend(new.into_iter().map(|(f, _)| f));
        Ok(c)
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
        let saved_dated = std::mem::take(&mut self.dated_vars);
        let v = self.calculate(&b, &[], ctx, rows);
        self.vars = saved;
        self.dated_vars = saved_dated;
        v
    }

    /// Whether an expression is a date by its type, as the SQL compiler types it (a
    /// blank date plus 3 is still a date/time, 1900-01-02).
    fn dated(&self, b: &Bound) -> bool {
        let column = |table: &str, column: &str| {
            self.model
                .table(table)
                .and_then(|t| t.column(column))
                .is_some_and(|c| {
                    matches!(
                        c.data_type,
                        charted_model::DataType::Date
                            | charted_model::DataType::DateTime
                            | charted_model::DataType::DateTimeTz
                            | charted_model::DataType::Time
                    )
                })
        };
        match b {
            Bound::Column { table, column: c }
            | Bound::Related { table, column: c }
            | Bound::Earlier {
                table, column: c, ..
            } => column(table, c),
            Bound::Info {
                func: InfoFunc::SelectedValue | InfoFunc::Lookup,
                table,
                column: Some(c),
                ..
            } => column(table, c),
            Bound::Measure { measure, .. } => self
                .model
                .tables
                .iter()
                .flat_map(|t| &t.measures)
                .find(|m| &m.id == measure)
                .and_then(|m| charted_dax::check(self.model, &m.expression).0)
                .is_some_and(|b| self.dated(&b)),
            Bound::Var(name) => self
                .dated_vars
                .iter()
                .rev()
                .find(|(n, _)| n == name)
                .is_some_and(|(_, d)| *d),
            Bound::Let { body, .. } => self.dated(body),
            Bound::Aggregate {
                func: AggFunc::Min | AggFunc::Max,
                arg: Some(a),
                ..
            } => self.dated(a),
            Bound::Calculate { expr, .. } => self.dated(expr),
            Bound::Call { func, args } => match func {
                ScalarFunc::Date
                | ScalarFunc::Today
                | ScalarFunc::UtcToday
                | ScalarFunc::Now
                | ScalarFunc::UtcNow
                | ScalarFunc::EoMonth
                | ScalarFunc::EDate => true,
                ScalarFunc::Lib(f) => matches!(
                    f.returns(),
                    charted_dax::Returns::Date | charted_dax::Returns::DateTime
                ),
                ScalarFunc::If => args.iter().skip(1).any(|a| self.dated(a)),
                ScalarFunc::Coalesce | ScalarFunc::Min | ScalarFunc::Max => {
                    args.iter().any(|a| self.dated(a))
                }
                _ => false,
            },
            Bound::Switch { cases, default, .. } => {
                cases.iter().any(|(_, t)| self.dated(t))
                    || default.as_deref().is_some_and(|d| self.dated(d))
            }
            Bound::Binary {
                op: BinaryOp::Add | BinaryOp::Sub,
                left,
                right,
            } => self.dated(left) || self.dated(right),
            _ => false,
        }
    }

    fn aggregate(
        &mut self,
        func: AggFunc,
        source: &TableExpr,
        arg: Option<&Bound>,
        param: Option<&Bound>,
        ctx: &Ctx,
        rows: &[Row],
    ) -> Result<Value> {
        let k = match param {
            Some(p) => self.eval(p, ctx, rows)?.numeric().unwrap_or(0.0),
            None => 0.0,
        };
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
            // In Power BI a NaN among numbers makes MIN and MAX NaN; NaNs alone are blank.
            AggFunc::Min | AggFunc::Max
                if nonblank.iter().any(|v| v.num().is_some_and(f64::is_nan)) =>
            {
                if nonblank.iter().all(|v| v.num().is_some_and(f64::is_nan)) {
                    Value::Blank
                } else {
                    Value::Num(f64::NAN)
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
            AggFunc::DistinctCountNoBlank => {
                let set: HashSet<Option<String>> = nonblank.iter().map(|v| v.key()).collect();
                if set.is_empty() {
                    Value::Blank
                } else {
                    Value::Num(set.len() as f64)
                }
            }
            AggFunc::Median | AggFunc::PercentileInc | AggFunc::PercentileExc => {
                // Blanks are in the list, first, as in Power BI: landing on one is
                // blank, and between values it is 0.
                let mut ns: Vec<Option<f64>> = vals.iter().map(|v| v.numeric()).collect();
                ns.sort_by(|a, b| match (a, b) {
                    (Some(x), Some(y)) => x.total_cmp(y),
                    _ => a.is_some().cmp(&b.is_some()),
                });
                let n = ns.len() as f64;
                let (pos, ok, name) = match func {
                    AggFunc::Median => ((n - 1.0) * 0.5, true, "MEDIAN"),
                    AggFunc::PercentileInc => {
                        (k * (n - 1.0), (0.0..=1.0).contains(&k), "PERCENTILE.INC")
                    }
                    _ => (
                        k * (n + 1.0) - 1.0,
                        k * (n + 1.0) >= 1.0 && k * (n + 1.0) <= n,
                        "PERCENTILE.EXC",
                    ),
                };
                if ns.is_empty() {
                    Value::Blank
                } else if !ok {
                    return Err(format!(
                        "An argument of function '{name}' has an invalid value."
                    ));
                } else {
                    let f = pos.floor() as usize;
                    let (lo, hi) = (ns[f], ns[(f + 1).min(ns.len() - 1)]);
                    if pos == f as f64 {
                        lo.map_or(Value::Blank, Value::Num)
                    } else if lo.is_none() && hi.is_none() {
                        Value::Blank
                    } else {
                        let (lo, hi) = (lo.unwrap_or(0.0), hi.unwrap_or(0.0));
                        Value::Num(lo + (pos - f as f64) * (hi - lo))
                    }
                }
            }
            AggFunc::StdevS | AggFunc::StdevP | AggFunc::VarS | AggFunc::VarP => {
                let ns: Vec<f64> = nonblank.iter().filter_map(|v| v.num()).collect();
                let n = ns.len() as f64;
                let sample = matches!(func, AggFunc::StdevS | AggFunc::VarS);
                let d = if sample { n - 1.0 } else { n };
                if n == 0.0 {
                    Value::Blank
                } else if d == 0.0 {
                    // A sample of one is 0/0 in Power BI.
                    Value::Num(f64::NAN)
                } else {
                    let mean = ns.iter().sum::<f64>() / n;
                    let var = ns.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / d;
                    Value::Num(if matches!(func, AggFunc::StdevS | AggFunc::StdevP) {
                        var.sqrt()
                    } else {
                        var
                    })
                }
            }
            AggFunc::Product => {
                let ns: Vec<f64> = nonblank.iter().filter_map(|v| v.num()).collect();
                if ns.is_empty() {
                    Value::Blank
                } else {
                    Value::Num(ns.iter().product())
                }
            }
            AggFunc::GeoMean => {
                let ns: Vec<f64> = nonblank.iter().filter_map(|v| v.num()).collect();
                if ns.is_empty() {
                    Value::Blank
                } else if ns.iter().any(|x| *x < 0.0) {
                    return Err("An argument of function 'GEOMEAN' has an invalid value.".into());
                } else if ns.contains(&0.0) {
                    Value::Num(0.0)
                } else {
                    Value::Num((ns.iter().map(|x| x.ln()).sum::<f64>() / ns.len() as f64).exp())
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
            // Column ids are unique across tables: a row may carry another table's
            // columns (SUMMARIZE's groups).
            Bound::Column { column, .. } => rows
                .iter()
                .rev()
                .find(|r| r.values.contains_key(column))
                .map(|r| r.values[column].clone())
                .unwrap_or(Value::Blank),
            Bound::Ext { name } => {
                let key = charted_dax::bind::ext_key(name);
                rows.iter()
                    .rev()
                    .find(|r| r.values.contains_key(&key))
                    .map(|r| r.values[&key].clone())
                    .unwrap_or(Value::Blank)
            }
            Bound::Earlier { table, column, n } => rows
                .iter()
                .rev()
                .filter(|r| &r.table == table && r.values.contains_key(column))
                .nth(*n)
                .map(|r| r.values[column].clone())
                .unwrap_or(Value::Blank),
            Bound::Path { table, id, parent } => {
                let Some(start) = rows
                    .iter()
                    .rev()
                    .find(|r| &r.table == table)
                    .and_then(|r| r.values.get(id))
                    .and_then(Value::key)
                else {
                    return Ok(Value::Blank);
                };
                let td = self.table(table)?;
                let (Some(ci), Some(pi)) = (td.col(id), td.col(parent)) else {
                    return Ok(Value::Blank);
                };
                let parents: HashMap<String, Option<String>> = td
                    .rows
                    .iter()
                    .filter_map(|r| {
                        let p = r[pi].key().filter(|p| !p.is_empty());
                        Some((r[ci].key()?, p))
                    })
                    .collect();
                // Up to a row with no parent, or to a parent that isn't a row (it heads
                // the path); going round a loop is an error, as in Power BI.
                let mut path = vec![start.clone()];
                let mut at = start;
                while let Some(Some(p)) = parents.get(&at) {
                    if path.len() > parents.len() {
                        return Err(crate::visual::path_loop(self.model, table, id, parent));
                    }
                    path.push(p.clone());
                    at = p.clone();
                }
                path.reverse();
                Value::Text(path.join("|"))
            }
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
                let dated = self.dated(value);
                self.vars.push((name.clone(), v));
                self.dated_vars.push((name.clone(), dated));
                let out = self.eval(body, ctx, rows);
                self.vars.pop();
                self.dated_vars.pop();
                out?
            }
            Bound::Aggregate {
                func,
                source,
                arg,
                param,
            } => self.aggregate(*func, source, arg.as_deref(), param.as_deref(), ctx, rows)?,
            Bound::ConcatX {
                source,
                expr,
                delimiter,
                order,
            } => {
                let mut parts = Vec::new();
                for r in self.eval_table(source, ctx, rows)? {
                    let mut stack = rows.to_vec();
                    stack.push(r);
                    let v = self.eval(expr, ctx, &stack)?;
                    let o = match order {
                        Some((o, _)) => self.eval(o, ctx, &stack)?,
                        None => Value::Blank,
                    };
                    parts.push((o, text(&v)));
                }
                if parts.is_empty() {
                    Value::Blank
                } else {
                    // The iterated rows are data (the visual shows the row).
                    self.touched = true;
                    let like = parts
                        .iter()
                        .map(|(o, _)| o)
                        .find(|o| !o.is_blank())
                        .cloned()
                        .unwrap_or(Value::Num(0.0));
                    let asc = order.as_ref().is_none_or(|(_, a)| *a);
                    parts.sort_by(|(oa, ta), (ob, tb)| {
                        let o = cmp(&coerced(oa, &like), &coerced(ob, &like));
                        (if asc { o } else { o.reverse() })
                            .then_with(|| ta.to_lowercase().cmp(&tb.to_lowercase()))
                            .then_with(|| ta.cmp(tb))
                    });
                    Value::Text(
                        parts
                            .into_iter()
                            .map(|(_, t)| t)
                            .collect::<Vec<_>>()
                            .join(delimiter),
                    )
                }
            }
            Bound::Rank {
                source,
                expr,
                value,
                ascending,
                dense,
            } => {
                let mut values = Vec::new();
                for r in self.eval_table(source, ctx, rows)? {
                    let mut stack = rows.to_vec();
                    stack.push(r);
                    values.push(self.eval(expr, ctx, &stack)?);
                }
                let v = self.eval(value.as_deref().unwrap_or(expr), ctx, rows)?;
                // Blank ranks as zero (or empty text).
                let like = values.iter().chain([&v]).find(|x| !x.is_blank()).cloned();
                let like = like.unwrap_or(Value::Num(0.0));
                let v = coerced(&v, &like);
                let beats = |x: &Value| {
                    let o = cmp(&coerced(x, &like), &v);
                    if *ascending { o.is_lt() } else { o.is_gt() }
                };
                let mut beating: Vec<Value> = values.into_iter().filter(|x| beats(x)).collect();
                if *dense {
                    beating.sort_by(|a, b| cmp(&coerced(a, &like), &coerced(b, &like)));
                    beating.dedup_by(|a, b| cmp(&coerced(a, &like), &coerced(b, &like)).is_eq());
                }
                Value::Num(1.0 + beating.len() as f64)
            }
            Bound::Calculate { expr, filters } => self.calculate(expr, filters, ctx, rows)?,
            Bound::Call { func, args } => {
                let mut a = args
                    .iter()
                    .map(|x| self.eval(x, ctx, rows))
                    .collect::<Result<Vec<_>>>()?;
                // ISNUMBER, ISTEXT…: the type, dates known from the expression (they
                // are text here).
                if let ScalarFunc::Lib(f) = func
                    && f.kind(0) == ArgKind::TypeOf
                {
                    a[0] = Value::Text(
                        match &a[0] {
                            Value::Blank => "blank",
                            // A date is a number to ISNUMBER, as in Power BI.
                            _ if self.dated(&args[0]) => "number",
                            Value::Num(_) => "number",
                            Value::Bool(_) => "logical",
                            _ => "text",
                        }
                        .into(),
                    );
                }
                match func {
                    ScalarFunc::Lib(f) => lib_fn(*f, &a)?,
                    ScalarFunc::Divide => match (a[0].numeric(), a[1].numeric()) {
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
                    ScalarFunc::Coalesce => a
                        .into_iter()
                        .find(|v| !v.is_blank())
                        .unwrap_or(Value::Blank),
                    ScalarFunc::And => Value::Bool(a[0].truthy() && a[1].truthy()),
                    ScalarFunc::Or => Value::Bool(a[0].truthy() || a[1].truthy()),
                    ScalarFunc::Not => Value::Bool(!a[0].truthy()),
                    ScalarFunc::IsBlank => Value::Bool(a[0].is_blank()),
                    ScalarFunc::Year
                    | ScalarFunc::Month
                    | ScalarFunc::Day
                    | ScalarFunc::Quarter => match day(&a[0]) {
                        Some(z) => {
                            let (y, m, d) = cal::civil(z);
                            Value::Num(match func {
                                ScalarFunc::Year => y as f64,
                                ScalarFunc::Month => m as f64,
                                ScalarFunc::Day => d as f64,
                                _ => ((m - 1) / 3 + 1) as f64,
                            })
                        }
                        None => Value::Blank,
                    },
                    other => scalar(other, &a)?,
                }
            }
            Bound::Info {
                func,
                table,
                column,
                alternate,
            } => {
                match func {
                    InfoFunc::SelectedValue | InfoFunc::HasOneValue | InfoFunc::Lookup => {
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
                        if *func == InfoFunc::Lookup && vals.len() > 1 && alternate.is_none() {
                            return Err("LOOKUPVALUE found more than one value".into());
                        }
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
                    UnaryOp::Neg => v.numeric().map(|x| Value::Num(-x)).unwrap_or(Value::Blank),
                    UnaryOp::Plus => v,
                    UnaryOp::Not => Value::Bool(!v.truthy()),
                }
            }
            Bound::Binary { op, left, right } => {
                let l = self.eval(left, ctx, rows)?;
                let r = self.eval(right, ctx, rows)?;
                if matches!(op, BinaryOp::Add | BinaryOp::Sub)
                    && (self.dated(left) || self.dated(right))
                {
                    date_add(*op, &l, &r)
                } else {
                    binary(*op, &l, &r)
                }
            }
        })
    }
}

/// Adding to or subtracting from a date gives a date/time (blank is day 0), in whole
/// microseconds as the SQL does.
fn date_add(op: BinaryOp, l: &Value, r: &Value) -> Value {
    if l.is_blank() && r.is_blank() {
        return Value::Blank;
    }
    let us = |v: &Value| match v {
        Value::Text(t) => cal::micros(t).unwrap_or(0),
        _ => (v.numeric().unwrap_or(0.0) * 86_400_000_000.0).round() as i64,
    };
    Value::Text(cal::format_micros(if op == BinaryOp::Add {
        us(l) + us(r)
    } else {
        us(l) - us(r)
    }))
}

fn binary(op: BinaryOp, l: &Value, r: &Value) -> Value {
    let n = |v: &Value| v.numeric().unwrap_or(0.0);
    match op {
        BinaryOp::Add | BinaryOp::Sub if l.serial().is_some() || r.serial().is_some() => {
            date_add(op, l, r)
        }
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
        // A blank numerator gives blank; otherwise a blank divisor is 0 and division
        // is IEEE: 1/0 is Infinity, 0/0 NaN (as in Power BI).
        BinaryOp::Div => {
            if l.is_blank() {
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
        BinaryOp::Concat => Value::Text(format!("{}{}", text(l), text(r))),
        BinaryOp::StrictEq => Value::Bool(l == r),
        BinaryOp::And => Value::Bool(l.truthy() && r.truthy()),
        BinaryOp::Or => Value::Bool(l.truthy() || r.truthy()),
        BinaryOp::In => Value::Blank,
        cmpop => {
            let (a, b) = (coerced(l, r), coerced(r, l));
            // Comparing with NaN is false in Power BI (only <> holds).
            if [&a, &b].iter().any(|v| v.num().is_some_and(f64::is_nan)) {
                return Value::Bool(cmpop == BinaryOp::Ne);
            }
            let o = cmp(&a, &b);
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

/// A date value (dates are ISO text here, as DuckDB writes them) as days since 1970.
fn day(v: &Value) -> Option<i64> {
    match v {
        Value::Text(t) => cal::parse(t),
        _ => None,
    }
}

/// Text as the text functions see it: blank is empty.
/// A value as DAX text (numbers as `general_number` writes them).
fn text(v: &Value) -> String {
    match v {
        Value::Num(x) => charted_dax::format::general_number(*x),
        // TRUE and FALSE in capitals, as Power BI writes them.
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        _ => v.key().unwrap_or_default(),
    }
}

/// A whole-number argument: blank counts as zero, decimals are truncated.
fn whole(v: &Value) -> i64 {
    v.numeric().unwrap_or(0.0).trunc() as i64
}

/// The text, math and date functions (the logical ones are evaluated in `eval`).
/// Today as days since 1970-01-01 (UTC; DuckDB's `current_date` is local, which only
/// differs around midnight).
fn now_micros() -> i64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_micros() as i64 + 25_569 * 86_400_000_000)
            .unwrap_or(0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        25_569 * 86_400_000_000
    }
}

fn today() -> i64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| (d.as_secs() / 86_400) as i64)
            .unwrap_or(0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
}

/// Arguments read as their kinds say (see `charted_dax::ArgKind`), the way the SQL
/// compiler's `lib_arg` reads them. None when a `NumOrBlank` argument is blank (or
/// isn't a number), which makes the result blank.
fn lib_args(f: LibFn, a: &[Value]) -> Option<Vec<Value>> {
    a.iter()
        .enumerate()
        .map(|(i, v)| match f.kind(i) {
            ArgKind::Num | ArgKind::NumBlank => Some(Value::Num(v.numeric().unwrap_or(0.0))),
            ArgKind::NumOrBlank => v.numeric().map(Value::Num),
            ArgKind::Int => Some(Value::Num(v.numeric().unwrap_or(0.0).round())),
            ArgKind::Text => Some(Value::Text(text(v))),
            ArgKind::Any | ArgKind::TypeOf => Some(v.clone()),
            ArgKind::Moment | ArgKind::Day0 => match v {
                Value::Text(t) => cal::micros(t).map(|u| Value::Num(u as f64)),
                Value::Blank if f.kind(i) == ArgKind::Day0 => Some(Value::Num(0.0)),
                Value::Blank => None,
                _ => v
                    .numeric()
                    .map(|n| Value::Num((n * 86_400_000_000.0).round())),
            },
            ArgKind::TextOrBlank => match v {
                Value::Bool(_) => Some(Value::Text(text(v))),
                _ => v.key().map(Value::Text),
            },
        })
        .collect()
}

/// YEARFRAC basis 1 of serial days `lo` <= `hi` (SQL's `actual_actual_sql`).
fn actual_actual(lo: i64, hi: i64) -> f64 {
    let civil = |z: i64| cal::civil(z - 25_569);
    let leap = |y: i64| (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let ((y1, m1, d1), (y2, m2, d2)) = (civil(lo), civil(hi));
    let days = (hi - lo) as f64;
    if y1 == y2 || (y2 == y1 + 1 && (m1 > m2 || (m1 == m2 && d1 >= d2))) {
        let den = if y1 == y2 {
            if leap(y1) { 366.0 } else { 365.0 }
        } else if (leap(y1) && m1 < 3) || (leap(y2) && (m2 > 2 || (m2 == 2 && d2 == 29))) {
            366.0
        } else {
            365.0
        };
        days / den
    } else {
        let span = (cal::days(y2 + 1, 1, 1) - cal::days(y1, 1, 1)) as f64;
        days / (span / (y2 - y1 + 1) as f64)
    }
}

/// NETWORKDAYS: SQL's `work_days_sql`, from serial day `lo` to `hi` (`lo` <= `hi`).
fn work_days(lo: i64, hi: i64, w: i64) -> Option<i64> {
    let n = hi - lo + 1;
    let dow = (lo + 6) % 7;
    let hit = |x: i64| i64::from(((x - dow) % 7 + 7) % 7 < n % 7);
    match w {
        1..=7 => Some(n - (n / 7) * 2 - hit((w + 5) % 7) - hit((w + 6) % 7)),
        11..=17 => Some(n - n / 7 - hit(w - 11)),
        _ => None,
    }
}

/// TIMEVALUE's text: `H:MM[:SS[.f]]` with an optional AM or PM, or an ISO date and
/// time; the time of day in microseconds.
fn time_of(t: &str) -> Option<i64> {
    let t = t.trim();
    if let Some(us) = cal::micros(t) {
        return Some(us.rem_euclid(86_400_000_000));
    }
    let upper = t.to_uppercase();
    let (clock, pm) = match upper
        .strip_suffix("PM")
        .or_else(|| upper.strip_suffix("AM"))
    {
        Some(c) => (c.trim_end().to_string(), Some(upper.ends_with("PM"))),
        None => (upper.clone(), None),
    };
    let mut it = clock.split(':');
    let h: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let sec: f64 = match it.next() {
        Some(s) => s.parse().ok()?,
        None => 0.0,
    };
    if it.next().is_some() || !(0..60).contains(&m) || !(0.0..60.0).contains(&sec) {
        return None;
    }
    let h = match pm {
        Some(pm) if (1..=12).contains(&h) => h % 12 + if pm { 12 } else { 0 },
        Some(_) => return None,
        None if (0..24).contains(&h) => h,
        None => return None,
    };
    Some((h * 3600 + m * 60) * 1_000_000 + (sec * 1e6).round() as i64)
}

/// DATEVALUE's text: an ISO date (and time) or `m/d/yyyy`; the serial day.
fn date_of(t: &str) -> Option<i64> {
    let t = t.trim();
    if let Some(us) = cal::micros(t) {
        return Some(us.div_euclid(86_400_000_000));
    }
    let mut it = t.split('/');
    let m: u32 = it.next()?.parse().ok()?;
    let d: u32 = it.next()?.parse().ok()?;
    let y: i64 = it.next()?.parse().ok()?;
    let z = cal::days(y, m, d);
    (it.next().is_none() && cal::civil(z) == (y, m, d)).then_some(z + 25_569)
}

/// The date and time functions; Moment arguments are microseconds since 1899-12-30
/// (SQL's `time_template`).
fn time_fn(f: LibFn, a: &[Value]) -> Result<Value> {
    const DAY: i64 = 86_400_000_000;
    let us = |i: usize| a[i].num().unwrap_or(0.0) as i64;
    let num = |i: usize| a.get(i).and_then(Value::num);
    let serial = |i: usize| us(i).div_euclid(DAY);
    let civil = |z: i64| cal::civil(z - 25_569);
    let bad = || {
        Err(format!(
            "An argument of function '{}' has an invalid value.",
            f.name()
        ))
    };
    let not_date = |t: &str| {
        Err(format!(
            "Cannot convert value '{t}' of type Text to type Date."
        ))
    };
    let tod = |i: usize| us(i).rem_euclid(DAY);
    Ok(match f {
        LibFn::Hour => Value::Num((tod(0) / 3_600_000_000) as f64),
        LibFn::Minute => Value::Num((tod(0) / 60_000_000 % 60) as f64),
        LibFn::Second => Value::Num((tod(0) / 1_000_000 % 60) as f64),
        LibFn::Time => {
            let total = num(0).unwrap_or(0.0) * 3600.0
                + num(1).unwrap_or(0.0) * 60.0
                + num(2).unwrap_or(0.0);
            if total < 0.0 {
                return bad();
            }
            Value::Text(cal::format_micros((total as i64 % 86_400) * 1_000_000))
        }
        LibFn::TimeValue => {
            let t = text(&a[0]);
            match time_of(&t) {
                Some(u) => Value::Text(cal::format_micros(u)),
                None => return not_date(&t),
            }
        }
        LibFn::DateValue => {
            let t = text(&a[0]);
            match date_of(&t) {
                Some(z) => Value::Text(cal::format(z - 25_569)),
                None => return not_date(&t),
            }
        }
        LibFn::WeekNum => {
            let z = serial(0);
            let t = num(1).map_or(1, |t| t as i64);
            let (y, _, _) = civil(z);
            if t == 21 {
                // ISO: the week of the Thursday of this week.
                let monday0 = (z + 5).rem_euclid(7);
                let thursday = z - monday0 + 3;
                let (ty, _, _) = civil(thursday);
                let jan1 = cal::days(ty, 1, 1) + 25_569;
                return Ok(Value::Num(((thursday - jan1) / 7 + 1) as f64));
            }
            let start = match t {
                1 | 17 => 0,
                2 => 1,
                12..=16 => t - 10,
                11 => 1,
                _ => return bad(),
            };
            let jan1 = cal::days(y, 1, 1) + 25_569;
            let doy = z - jan1 + 1;
            let jan1_dow = (jan1 + 6) % 7;
            Value::Num(((doy - 1 + (jan1_dow - start + 7) % 7) / 7 + 1) as f64)
        }
        LibFn::YearFrac => {
            let (lo, hi) = (serial(0).min(serial(1)), serial(0).max(serial(1)));
            let ((y1, m1, d1), (y2, m2, d2)) = (civil(lo), civil(hi));
            let last_feb = |z: i64, m: u32| m == 2 && civil(z + 1).2 == 1;
            let thirty = |e1: i64, e2: i64| {
                ((y2 - y1) * 360 + (m2 as i64 - m1 as i64) * 30 + (e2 - e1)) as f64 / 360.0
            };
            match num(2).map_or(0, |b| b as i64) {
                0 => {
                    let e1 = if last_feb(lo, m1) || d1 == 31 {
                        30
                    } else {
                        d1 as i64
                    };
                    let e2 = if (last_feb(lo, m1) && last_feb(hi, m2)) || (d2 == 31 && d1 >= 30) {
                        30
                    } else {
                        d2 as i64
                    };
                    Value::Num(thirty(e1, e2))
                }
                2 => Value::Num((hi - lo) as f64 / 360.0),
                3 => Value::Num((hi - lo) as f64 / 365.0),
                4 => Value::Num(thirty(d1.min(30) as i64, d2.min(30) as i64)),
                1 => Value::Num(actual_actual(lo, hi)),
                _ => return bad(),
            }
        }
        LibFn::NetworkDays => {
            let (d1, d2) = (serial(0), serial(1));
            let w = num(2).map_or(1, |w| w as i64);
            let n = if d1 <= d2 {
                work_days(d1, d2, w)
            } else {
                work_days(d2, d1, w).map(|n| -n)
            };
            match n {
                Some(n) => Value::Num(n as f64),
                None => return bad(),
            }
        }
        _ => unreachable!("{} isn't a date function", f.name()),
    })
}

/// SEARCH: the 0-based character position of the first match of `pattern` in `text`,
/// ignoring case; `?` is any character, `*` any run, `~` makes the next one literal
/// (SQL's `wildcard_regex`).
fn wildcard_find(text: &str, pattern: &str) -> Option<usize> {
    #[derive(Clone, Copy)]
    enum Tok {
        Lit(char),
        One,
        Many,
    }
    let mut toks = Vec::new();
    let mut it = pattern.chars().peekable();
    while let Some(c) = it.next() {
        toks.push(match c {
            '~' if matches!(it.peek(), Some('~' | '?' | '*')) => Tok::Lit(it.next().unwrap()),
            '?' => Tok::One,
            '*' => Tok::Many,
            c => Tok::Lit(c),
        });
    }
    let fold = |c: char| c.to_lowercase().next().unwrap_or(c);
    let chars: Vec<char> = text.chars().map(fold).collect();
    fn at(t: &[char], p: &[Tok], fold: &dyn Fn(char) -> char) -> bool {
        match p.split_first() {
            None => true,
            Some((Tok::Many, rest)) => (0..=t.len()).any(|i| at(&t[i..], rest, fold)),
            Some((Tok::One, rest)) => !t.is_empty() && at(&t[1..], rest, fold),
            Some((Tok::Lit(c), rest)) => t.first() == Some(&fold(*c)) && at(&t[1..], rest, fold),
        }
    }
    (0..=chars.len()).find(|&i| at(&chars[i..], &toks, &fold))
}

/// DuckDB's `round(v, 9)`: ratios are snapped before rounding up or down.
fn snap(v: f64) -> f64 {
    (v * 1e9).round() / 1e9
}

/// DuckDB's `sign`: 0 for 0.
fn sgn(v: f64) -> f64 {
    if v > 0.0 {
        1.0
    } else if v < 0.0 {
        -1.0
    } else {
        0.0
    }
}

fn gcd(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.abs()
}

/// A pseudo-random number in [0, 1) (RAND; never compared with SQL's).
fn random() -> f64 {
    use std::cell::Cell;
    thread_local!(static STATE: Cell<u64> = Cell::new(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1)
            | 1
    ));
    STATE.with(|s| {
        let mut x = s.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        (x >> 11) as f64 / (1u64 << 53) as f64
    })
}

/// The math library functions, with the same formulas as SQL's (math_template).
/// `blank`: the first argument was blank (and is 0 here); LN of it is -Infinity, not
/// an error, as in Power BI.
fn math_fn(f: LibFn, a: &[f64], blank: bool) -> Result<Value> {
    use std::f64::consts::PI;
    let x = a.first().copied().unwrap_or(0.0);
    let opt = |i: usize, default: f64| a.get(i).copied().unwrap_or(default);
    let bad = || {
        Err(format!(
            "An argument of function '{}' has an invalid value.",
            f.name()
        ))
    };
    let div0 = || {
        Err(format!(
            "Division by zero has occurred when evaluating function '{}'.",
            f.name()
        ))
    };
    let sinh = |x: f64| (x.exp() - (-x).exp()) / 2.0;
    let cosh = |x: f64| (x.exp() + (-x).exp()) / 2.0;
    Ok(Value::Num(match f {
        LibFn::Ceiling | LibFn::Floor => {
            let s = a[1];
            if s == 0.0 && f == LibFn::Floor {
                return bad();
            } else if s == 0.0 {
                0.0
            } else if x > 0.0 && s < 0.0 {
                return bad();
            } else if f == LibFn::Ceiling {
                s * snap(x / s).ceil()
            } else {
                s * snap(x / s).floor()
            }
        }
        LibFn::IsoCeiling => {
            let s = opt(1, 1.0);
            if s == 0.0 {
                0.0
            } else {
                s.abs() * snap(x / s.abs()).ceil()
            }
        }
        LibFn::RoundUp | LibFn::RoundDown | LibFn::Trunc => {
            let m = 10f64.powf(opt(1, 0.0).trunc());
            let v = snap(x.abs() * m);
            let r = if f == LibFn::RoundUp {
                v.ceil()
            } else {
                v.floor()
            };
            sgn(x) * r / m
        }
        LibFn::MRound => {
            let m = a[1];
            if m == 0.0 {
                0.0
            } else if (x > 0.0 && m < 0.0) || (x < 0.0 && m > 0.0) {
                return bad();
            } else {
                m * snap(x / m).round()
            }
        }
        LibFn::Even => sgn(x) * (x.abs() / 2.0).ceil() * 2.0,
        LibFn::Odd => {
            let v = ((x.abs() + 1.0) / 2.0).ceil() * 2.0 - 1.0;
            if x < 0.0 { -v } else { v }
        }
        LibFn::Sqrt | LibFn::SqrtPi if x < 0.0 => return bad(),
        LibFn::Sqrt => x.sqrt(),
        LibFn::SqrtPi => (x * PI).sqrt(),
        LibFn::Power => {
            let y = a[1];
            let root = 1.0 / y;
            if x == 0.0 && y < 0.0 {
                return bad();
            } else if x < 0.0
                && y != y.trunc()
                && (root - root.round()).abs() < 1e-9
                && (root.round() as i64) % 2 != 0
            {
                -(-x).powf(y)
            } else {
                x.powf(y)
            }
        }
        LibFn::Ln | LibFn::Log10 if blank => f64::NEG_INFINITY,
        LibFn::Log if blank => {
            let b = opt(1, 10.0);
            if b <= 0.0 || b == 1.0 {
                return bad();
            }
            f64::NEG_INFINITY / b.ln()
        }
        LibFn::Exp => x.exp(),
        LibFn::Ln | LibFn::Log10 if x <= 0.0 => return bad(),
        LibFn::Ln => x.ln(),
        LibFn::Log10 => x.log10(),
        LibFn::Log => {
            let b = opt(1, 10.0);
            if x <= 0.0 || b <= 0.0 || b == 1.0 {
                return bad();
            }
            x.ln() / b.ln()
        }
        LibFn::Sign => sgn(x),
        LibFn::Quotient => {
            if a[1] == 0.0 {
                return div0();
            }
            (x / a[1]).trunc()
        }
        LibFn::Fact => {
            if !(0.0..171.0).contains(&x) {
                return bad();
            }
            (1..=x.trunc() as i64).fold(1.0, |p, i| p * i as f64)
        }
        LibFn::Gcd | LibFn::Lcm => {
            let y = a[1];
            if x < 0.0 || y < 0.0 {
                return bad();
            }
            let (p, q) = (x.trunc() as i64, y.trunc() as i64);
            let g = gcd(p, q);
            if f == LibFn::Gcd {
                g as f64
            } else if p == 0 || q == 0 {
                0.0
            } else {
                (p / g * q).abs() as f64
            }
        }
        LibFn::Degrees => x * 180.0 / PI,
        LibFn::Radians => x * PI / 180.0,
        LibFn::Sin => x.sin(),
        LibFn::Cos => x.cos(),
        LibFn::Tan => x.tan(),
        LibFn::Cot if x == 0.0 => return div0(),
        LibFn::Cot => 1.0 / x.tan(),
        LibFn::Asin | LibFn::Acos if x.abs() > 1.0 => return bad(),
        LibFn::Asin => x.asin(),
        LibFn::Acos => x.acos(),
        LibFn::Atan => x.atan(),
        LibFn::Acot => PI / 2.0 - x.atan(),
        LibFn::Sinh => sinh(x),
        LibFn::Cosh => cosh(x),
        LibFn::Tanh | LibFn::Coth if x > 20.0 => 1.0,
        LibFn::Tanh | LibFn::Coth if x < -20.0 => -1.0,
        LibFn::Tanh => sinh(x) * 2.0 / (cosh(x) * 2.0),
        LibFn::Coth if x == 0.0 => return div0(),
        LibFn::Coth => cosh(x) * 2.0 / (sinh(x) * 2.0),
        LibFn::Asinh => (x + (x * x + 1.0).sqrt()).ln(),
        LibFn::Acosh if x < 1.0 => return bad(),
        LibFn::Acosh => (x + (x * x - 1.0).sqrt()).ln(),
        LibFn::Atanh if x.abs() >= 1.0 => return bad(),
        LibFn::Atanh => 0.5 * ((1.0 + x) / (1.0 - x)).ln(),
        LibFn::Acoth if x.abs() <= 1.0 => return bad(),
        LibFn::Acoth => 0.5 * ((x + 1.0) / (x - 1.0)).ln(),
        LibFn::Currency => (x * 1e4).round() / 1e4,
        LibFn::Rand => random(),
        LibFn::RandBetween => (random() * (a[1] - x + 1.0)).floor() + x,
        _ => unreachable!("{} isn't a math function", f.name()),
    }))
}

/// Each library function, its arguments read by `lib_args`.
fn lib_fn(f: LibFn, a: &[Value]) -> Result<Value> {
    // A blank first argument: the function of 0, and blank if that is 0.
    let blank = f.kind(0) == ArgKind::NumBlank && a.first().is_some_and(Value::is_blank);
    // Text that isn't a number, where a number is wanted, is an error (SQL's cast
    // fails the same way).
    for (i, v) in a.iter().enumerate() {
        if let (
            ArgKind::Num | ArgKind::NumOrBlank | ArgKind::NumBlank | ArgKind::Int,
            Value::Text(t),
        ) = (f.kind(i), v)
            && v.numeric().is_none()
        {
            return Err(format!(
                "Cannot convert value '{t}' of type Text to type Number."
            ));
        }
    }
    // Text that isn't a date, where a date is wanted, is an error.
    for (i, v) in a.iter().enumerate() {
        if let (ArgKind::Moment | ArgKind::Day0, Value::Text(t)) = (f.kind(i), v)
            && cal::micros(t).is_none()
        {
            return Err(format!(
                "Cannot convert value '{t}' of type Text to type Date."
            ));
        }
    }
    let Some(a) = lib_args(f, a) else {
        return Ok(Value::Blank);
    };
    let num = |i: usize| a[i].num().unwrap_or(0.0);
    let int = |i: usize| num(i) as i64;
    let txt = |i: usize| text(&a[i]);
    let bad = || {
        Err(format!(
            "An argument of function '{}' has an invalid value.",
            f.name()
        ))
    };
    Ok(match f {
        LibFn::Ceiling
        | LibFn::Floor
        | LibFn::IsoCeiling
        | LibFn::RoundUp
        | LibFn::RoundDown
        | LibFn::Trunc
        | LibFn::MRound
        | LibFn::Even
        | LibFn::Odd
        | LibFn::Sqrt
        | LibFn::SqrtPi
        | LibFn::Power
        | LibFn::Exp
        | LibFn::Ln
        | LibFn::Log
        | LibFn::Log10
        | LibFn::Sign
        | LibFn::Quotient
        | LibFn::Fact
        | LibFn::Gcd
        | LibFn::Lcm
        | LibFn::Degrees
        | LibFn::Radians
        | LibFn::Sin
        | LibFn::Cos
        | LibFn::Tan
        | LibFn::Cot
        | LibFn::Asin
        | LibFn::Acos
        | LibFn::Atan
        | LibFn::Acot
        | LibFn::Sinh
        | LibFn::Cosh
        | LibFn::Tanh
        | LibFn::Coth
        | LibFn::Asinh
        | LibFn::Acosh
        | LibFn::Atanh
        | LibFn::Acoth
        | LibFn::Currency
        | LibFn::Rand
        | LibFn::RandBetween => {
            let ns: Vec<f64> = (0..a.len()).map(num).collect();
            return Ok(match math_fn(f, &ns, blank)? {
                Value::Num(n) if blank && n == 0.0 => Value::Blank,
                v => v,
            });
        }
        LibFn::Abs => Value::Num(num(0).abs()),
        LibFn::Int => Value::Num(num(0).floor()),
        LibFn::Round => {
            let m = 10f64.powi(num(1) as i32);
            Value::Num((num(0) * m).round() / m)
        }
        LibFn::Mod => {
            let (x, d) = (num(0), num(1));
            if d == 0.0 {
                return Err("Division by zero has occurred when evaluating function 'MOD'.".into());
            }
            Value::Num(x - d * (x / d).floor())
        }
        LibFn::Value => {
            let raw = txt(0);
            let t = raw.trim();
            let plain = t.strip_prefix('$').unwrap_or(t).replace(',', "");
            match plain.parse::<f64>() {
                Ok(n) if !plain.is_empty() => Value::Num(n),
                _ => match cal::parse(t).filter(|_| t.len() == 10) {
                    Some(d) => Value::Num((d + 25_569) as f64),
                    None => {
                        return Err(format!(
                            "Cannot convert value '{raw}' of type Text to type Number."
                        ));
                    }
                },
            }
        }
        LibFn::PathLength => Value::Num(txt(0).split('|').count() as f64),
        LibFn::PathItem | LibFn::PathItemReverse => {
            let path = txt(0);
            let parts: Vec<&str> = path.split('|').collect();
            let n = int(1);
            if n < 1 || n as usize > parts.len() {
                Value::Blank
            } else if f == LibFn::PathItem {
                Value::Text(parts[n as usize - 1].to_string())
            } else {
                Value::Text(parts[parts.len() - n as usize].to_string())
            }
        }
        LibFn::PathContains => Value::Bool(txt(0).split('|').any(|p| p == txt(1))),
        LibFn::Upper => Value::Text(txt(0).to_uppercase()),
        LibFn::Lower => Value::Text(txt(0).to_lowercase()),
        LibFn::Len => Value::Num(txt(0).chars().count() as f64),
        // Leading and trailing spaces go; runs of spaces inside become one.
        LibFn::Trim => Value::Text(
            txt(0)
                .split(' ')
                .filter(|p| !p.is_empty())
                .collect::<Vec<_>>()
                .join(" "),
        ),
        LibFn::Left | LibFn::Right => {
            let n = if a.len() > 1 { int(1) } else { 1 };
            if n < 0 {
                return bad();
            }
            let n = n as usize;
            let chars: Vec<char> = txt(0).chars().collect();
            let cut = if f == LibFn::Left {
                &chars[..n.min(chars.len())]
            } else {
                &chars[chars.len().saturating_sub(n)..]
            };
            Value::Text(cut.iter().collect())
        }
        LibFn::Hour
        | LibFn::Minute
        | LibFn::Second
        | LibFn::Time
        | LibFn::TimeValue
        | LibFn::DateValue
        | LibFn::WeekNum
        | LibFn::YearFrac
        | LibFn::NetworkDays => return time_fn(f, &a),
        LibFn::IsNumber => Value::Bool(txt(0) == "number"),
        LibFn::IsText => Value::Bool(txt(0) == "text"),
        LibFn::IsNonText => Value::Bool(txt(0) != "text"),
        LibFn::IsLogical => Value::Bool(txt(0) == "logical"),
        LibFn::IsEven => Value::Bool(num(0).trunc() % 2.0 == 0.0),
        LibFn::IsOdd => Value::Bool(num(0).trunc() % 2.0 != 0.0),
        LibFn::BitAnd | LibFn::BitOr | LibFn::BitXor | LibFn::BitLShift | LibFn::BitRShift => {
            let (x, n) = (num(0).trunc() as i64, num(1).trunc() as i64);
            let shift = |x: i64, n: i64, left: bool| {
                if (n >= 0) == left {
                    x.wrapping_shl(n.unsigned_abs() as u32)
                } else {
                    x.wrapping_shr(n.unsigned_abs() as u32)
                }
            };
            Value::Num(match f {
                LibFn::BitAnd => x & n,
                LibFn::BitOr => x | n,
                LibFn::BitXor => x ^ n,
                LibFn::BitLShift => shift(x, n, true),
                _ => shift(x, n, false),
            } as f64)
        }
        LibFn::Find | LibFn::Search => {
            let (find, within) = (txt(0), txt(1));
            let start = if a.len() > 2 { int(2) } else { 1 };
            let chars: Vec<char> = within.chars().collect();
            if start < 1 || start as usize > chars.len() + 1 {
                return bad();
            }
            let rest: String = chars[start as usize - 1..].iter().collect();
            let at = if f == LibFn::Find {
                rest.find(&find).map(|b| rest[..b].chars().count())
            } else {
                wildcard_find(&rest, &find)
            };
            match (at, a.get(3)) {
                (Some(i), _) => Value::Num((i as i64 + start) as f64),
                (None, Some(v)) => v.clone(),
                (None, None) => {
                    return Err(format!(
                        "The search Text provided to function '{}' could not be found in the given text.",
                        f.name()
                    ));
                }
            }
        }
        // Nothing is found in empty text, not even empty text.
        LibFn::ContainsString => {
            Value::Bool(!txt(0).is_empty() && wildcard_find(&txt(0), &txt(1)).is_some())
        }
        LibFn::ContainsStringExact => Value::Bool(txt(0).contains(&txt(1))),
        LibFn::Exact => Value::Bool(txt(0) == txt(1)),
        LibFn::Replace => {
            let (start, n) = (int(1), int(2));
            if start < 1 || n < 0 {
                return bad();
            }
            let chars: Vec<char> = txt(0).chars().collect();
            let head: String = chars.iter().take(start as usize - 1).collect();
            let tail: String = chars.iter().skip((start + n) as usize - 1).collect();
            Value::Text(format!("{head}{}{tail}", txt(3)))
        }
        LibFn::Rept => {
            let n = num(1);
            if n < 0.0 {
                return bad();
            }
            Value::Text(txt(0).repeat(n.trunc() as usize))
        }
        // The same arithmetic as SQL's (text_template).
        LibFn::Fixed => {
            let x = num(0);
            let d = if a.len() > 1 { num(1) } else { 2.0 };
            let plain = a.len() > 2 && num(2) != 0.0;
            let p = 10f64.powf(d);
            let n = (x.abs() * p).round();
            let whole = if d > 0.0 { (n / p).trunc() } else { n / p };
            let mut digits = format!("{whole:.0}");
            if !plain {
                let mut grouped = String::new();
                for (i, c) in digits.chars().enumerate() {
                    if i > 0 && (digits.len() - i) % 3 == 0 {
                        grouped.push(',');
                    }
                    grouped.push(c);
                }
                digits = grouped;
            }
            let frac = if d > 0.0 {
                format!(".{:0>w$.0}", n - whole * p, w = d as usize)
            } else {
                String::new()
            };
            let sign = if x < 0.0 && n > 0.0 { "-" } else { "" };
            Value::Text(format!("{sign}{digits}{frac}"))
        }
        LibFn::Unichar => {
            let n = num(0);
            match char::from_u32(n.trunc() as u32).filter(|_| (1.0..=1_114_111.0).contains(&n)) {
                Some(c) => Value::Text(c.to_string()),
                None => return bad(),
            }
        }
        LibFn::Unicode => match txt(0).chars().next() {
            Some(c) => Value::Num(c as u32 as f64),
            None => Value::Blank,
        },
        LibFn::CombineValues => {
            Value::Text((1..a.len()).map(txt).collect::<Vec<_>>().join(&txt(0)))
        }
        LibFn::Mid => {
            let (start, n) = (int(1), int(2));
            if start < 1 || n < 0 {
                return bad();
            }
            Value::Text(
                txt(0)
                    .chars()
                    .skip(start as usize - 1)
                    .take(n as usize)
                    .collect(),
            )
        }
    })
}

fn scalar(func: &ScalarFunc, a: &[Value]) -> Result<Value> {
    let date = |z: i64| Value::Text(cal::format(z));
    Ok(match func {
        ScalarFunc::Lib(f) => lib_fn(*f, a)?,
        ScalarFunc::Min | ScalarFunc::Max => {
            let max = matches!(func, ScalarFunc::Max);
            let text_like = |v: &Value| matches!(v, Value::Text(_));
            if a[0].is_blank() && a[1].is_blank() {
                Value::Blank
            } else if !text_like(&a[0]) && !text_like(&a[1]) {
                // Numbers: blank compares as zero but comes back as blank when it wins;
                // on a tie the other one does (MAX ( BLANK (), -1 ) is blank).
                let (x, y) = (a[0].num().unwrap_or(0.0), a[1].num().unwrap_or(0.0));
                let first = if x == y {
                    a[1].is_blank()
                } else {
                    (x > y) == max
                };
                let (v, n) = if first { (&a[0], x) } else { (&a[1], y) };
                if v.is_blank() {
                    Value::Blank
                } else {
                    Value::Num(n)
                }
            } else if a[0].is_blank() || a[1].is_blank() {
                // Text and dates: the blank one is ignored.
                if a[0].is_blank() {
                    a[1].clone()
                } else {
                    a[0].clone()
                }
            } else if cmp(&a[0], &a[1]).is_lt() == max {
                a[1].clone()
            } else {
                a[0].clone()
            }
        }
        ScalarFunc::Substitute => {
            let (s, old, new) = (text(&a[0]), text(&a[1]), text(&a[2]));
            Value::Text(match a.get(3) {
                _ if old.is_empty() => s,
                None => s.replace(&old, &new),
                // Only the k-th occurrence.
                Some(i) => {
                    let k = whole(i);
                    let parts: Vec<&str> = s.split(old.as_str()).collect();
                    if k >= 1 && (k as usize) < parts.len() {
                        let k = k as usize;
                        format!("{}{new}{}", parts[..k].join(&old), parts[k..].join(&old))
                    } else {
                        s
                    }
                }
            })
        }
        ScalarFunc::Format(spec) => match (&a[0], spec) {
            (Value::Blank, _) => Value::Text(String::new()),
            (Value::Num(x), FormatSpec::General) => Value::Text(text(&Value::Num(*x))),
            (
                Value::Num(x),
                FormatSpec::Number {
                    decimals,
                    thousands,
                    percent,
                    prefix,
                },
            ) => Value::Text(format_number(*x, *decimals, *thousands, *percent, prefix)),
            (v @ Value::Text(_), FormatSpec::Date(parts)) => match day(v) {
                Some(z) => Value::Text(format_date(z, parts)),
                None => return Err("FORMAT with a date format needs a date".into()),
            },
            _ => return Err("FORMAT's value doesn't match its format string".into()),
        },
        ScalarFunc::Date => {
            // As Power BI: the parts are rounded, months and days roll over, but a day
            // of 0 or a year outside 1–9999 is an error.
            let part = |v: &Value| v.num().unwrap_or(0.0).round() as i64;
            let (y, m, d) = (part(&a[0]), part(&a[1]), part(&a[2]));
            let z = cal::date(y, m, d);
            if d == 0 || !(1..=9999).contains(&cal::civil(z).0) {
                return Err("An argument of function 'DATE' has the wrong data type or the result is too large or too small.".into());
            }
            date(z)
        }
        ScalarFunc::Today | ScalarFunc::UtcToday => match crate::clock::fixed() {
            Some(at) => Value::Text(at[..10].to_string()),
            None => date(today()),
        },
        // UTC: the interpreter has no time zones (the SQL's NOW is local time).
        ScalarFunc::Now | ScalarFunc::UtcNow => Value::Text(match crate::clock::fixed() {
            Some(at) => at,
            None => cal::format_micros(now_micros()),
        }),
        ScalarFunc::Weekday => match day(&a[0]) {
            // 1970-01-01 was a Thursday.
            Some(z) => {
                let sunday0 = (z + 4).rem_euclid(7);
                let monday1 = (z + 3).rem_euclid(7) + 1;
                Value::Num(match a.get(1).map(whole).unwrap_or(1) {
                    2 => monday1,
                    3 => monday1 - 1,
                    _ => sunday0 + 1,
                } as f64)
            }
            None => Value::Blank,
        },
        ScalarFunc::EoMonth => match day(&a[0]) {
            Some(z) => date(cal::last_day(cal::add(
                cal::trunc(z, Period::Month),
                whole(&a[1]) as i32,
                Period::Month,
            ))),
            None => Value::Blank,
        },
        ScalarFunc::EDate => match day(&a[0]) {
            Some(z) => date(cal::add(z, whole(&a[1]) as i32, Period::Month)),
            None => Value::Blank,
        },
        // The unit boundaries crossed, as Power BI counts them.
        ScalarFunc::DateDiff(p) => match (day(&a[0]), day(&a[1])) {
            (Some(x), Some(y)) => {
                let ((y1, m1, _), (y2, m2, _)) = (cal::civil(x), cal::civil(y));
                let months = |y: i64, m: u32| y * 12 + m as i64 - 1;
                Value::Num(match p {
                    Period::Day => y - x,
                    Period::Month => months(y2, m2) - months(y1, m1),
                    Period::Quarter => months(y2, m2).div_euclid(3) - months(y1, m1).div_euclid(3),
                    Period::Year => y2 - y1,
                } as f64)
            }
            _ => Value::Blank,
        },
        _ => return Err("unexpected scalar function".into()),
    })
}

/// A number with a fixed number of decimals (halves rounded away from zero), as FORMAT
/// writes it: the sign, the currency symbol, the digits, the percent sign.
fn format_number(x: f64, decimals: usize, thousands: bool, percent: bool, prefix: &str) -> String {
    let v = if percent { x * 100.0 } else { x };
    let m = 10f64.powi(decimals as i32);
    let r = (v * m).round() / m;
    let digits = format!("{:.decimals$}", r.abs());
    let digits = if thousands {
        let (int, frac) = digits.split_at(digits.find('.').unwrap_or(digits.len()));
        let mut grouped = String::new();
        for (i, c) in int.chars().enumerate() {
            if i > 0 && (int.len() - i) % 3 == 0 {
                grouped.push(',');
            }
            grouped.push(c);
        }
        grouped + frac
    } else {
        digits
    };
    format!(
        "{}{prefix}{digits}{}",
        if r < 0.0 { "-" } else { "" },
        if percent { "%" } else { "" }
    )
}

fn format_date(z: i64, parts: &[DatePart]) -> String {
    use charted_dax::format::{MONTHS, WEEKDAYS};
    let (y, m, d) = cal::civil(z);
    let month = MONTHS[m as usize - 1];
    // 1970-01-01 was a Thursday.
    let weekday = WEEKDAYS[(z + 4).rem_euclid(7) as usize];
    parts
        .iter()
        .map(|p| match p {
            DatePart::Year => format!("{y:04}"),
            DatePart::Year2 => format!("{:02}", y.rem_euclid(100)),
            DatePart::MonthName => month.to_string(),
            DatePart::MonthAbbr => month[..3].to_string(),
            DatePart::Month2 => format!("{m:02}"),
            DatePart::Month => m.to_string(),
            DatePart::DayName => weekday.to_string(),
            DatePart::DayAbbr => weekday[..3].to_string(),
            DatePart::Day2 => format!("{d:02}"),
            DatePart::Day => d.to_string(),
            DatePart::Literal(l) => l.clone(),
        })
        .collect()
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
            param: None,
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
    // Row-level security restricts the data itself.
    if !q.security.is_empty() {
        let secured = secure(model, data, &q.security)?;
        let open = VisualQuery {
            security: Vec::new(),
            ..q.clone()
        };
        return run(model, &secured, &open);
    }
    let mut it = Interp {
        model,
        data,
        vars: Vec::new(),
        dated_vars: Vec::new(),
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
                security: Vec::new(),
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
        // Rows where every value is blank are left out, as SUMMARIZECOLUMNS does; a
        // row with no data stays when a value isn't blank there (IF ( [Qty] > 10,
        // "High", "Low" ) shows "Low" for every month, as in Power BI).
        if !values.is_empty() && !groups.is_empty() && vals.iter().all(Value::is_blank) {
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
/// A calculated column's values, in the table's row order: the expression per row,
/// with the row as the row context and no filters.
pub fn calculated_column(
    model: &Model,
    data: &Data,
    table: &str,
    expression: &str,
) -> Result<Vec<Value>> {
    let (b, d) = charted_dax::check_in(model, expression, Some(table));
    let b = b.ok_or_else(|| d.first().map(|d| d.message.clone()).unwrap_or_default())?;
    let mut it = Interp {
        model,
        data,
        vars: Vec::new(),
        dated_vars: Vec::new(),
        touched: false,
    };
    let td = it.table(table)?;
    let mut out = Vec::new();
    for r in &td.rows {
        let row = it.to_row(table, r, None)?;
        out.push(it.eval(&b, &Ctx::default(), &[row])?);
    }
    Ok(out)
}

/// A calculated table's rows: the table expression evaluated with no filters, its
/// columns named as their model columns (repeats get 2, 3…).
pub fn calculated_table(model: &Model, data: &Data, expression: &str) -> Result<TableData> {
    let (t, d) = charted_dax::check_table(model, expression);
    let t = t.ok_or_else(|| d.first().map(|d| d.message.clone()).unwrap_or_default())?;
    let mut it = Interp {
        model,
        data,
        vars: Vec::new(),
        dated_vars: Vec::new(),
        touched: false,
    };
    let rows = it.eval_table(&t, &Ctx::default(), &[])?;
    let shape = t.shape(model);
    let mut names: Vec<String> = Vec::new();
    for (_, base) in &shape {
        let mut name = base.clone();
        let mut i = 1;
        while names.iter().any(|x| x.eq_ignore_ascii_case(&name)) {
            i += 1;
            name = format!("{base}{i}");
        }
        names.push(name);
    }
    Ok(TableData {
        columns: names,
        rows: rows
            .iter()
            .map(|r| {
                shape
                    .iter()
                    .map(|(k, _)| r.values.get(k).cloned().unwrap_or(Value::Blank))
                    .collect()
            })
            .collect(),
    })
}

/// The rows a role sees: each table's rows whose own row, and the rows they reach
/// through many-to-one relationships, pass the role's filters (a missing related row
/// doesn't).
fn secure(model: &Model, data: &Data, filters: &[crate::visual::RowFilter]) -> Result<Data> {
    let mut it = Interp {
        model,
        data,
        vars: Vec::new(),
        dated_vars: Vec::new(),
        touched: false,
    };
    let mut conds = Vec::new();
    for f in filters {
        let (b, d) = charted_dax::check_in(model, &f.expression, Some(&f.table));
        let b = b.ok_or_else(|| d.first().map(|d| d.message.clone()).unwrap_or_default())?;
        conds.push((f.table.clone(), b));
    }
    let many_to_one = |h: &Hop| {
        model
            .relationships
            .iter()
            .any(|r| r.from_column == h.from_col && r.to_column == h.to_col)
    };
    let mut tables = HashMap::new();
    for (tid, td) in &data.tables {
        let mut rows = Vec::new();
        'rows: for r in &td.rows {
            for (st, b) in &conds {
                let target = if st == tid {
                    Some(r.clone())
                } else {
                    match it.path(&Ctx::default(), tid, st) {
                        Some(hops) if hops.iter().all(many_to_one) => {
                            it.follow(tid, r, &hops)?.map(|(_, v)| v)
                        }
                        _ => continue,
                    }
                };
                let Some(target) = target else { continue 'rows };
                let row = it.to_row(st, &target, None)?;
                if !it.eval(b, &Ctx::default(), &[row])?.truthy() {
                    continue 'rows;
                }
            }
            rows.push(r.clone());
        }
        tables.insert(
            tid.clone(),
            TableData {
                columns: td.columns.clone(),
                rows,
            },
        );
    }
    Ok(Data { tables })
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

    /// ISO text (`2024-01-02`, `2024-01-02 03:04:05.5` or with a `T`) as a serial
    /// number; anything else is None.
    pub fn serial(s: &str) -> Option<f64> {
        micros(s).map(|us| us as f64 / 86_400_000_000.0)
    }

    /// ISO text as microseconds since 1899-12-30.
    pub fn micros(s: &str) -> Option<i64> {
        let b = s.as_bytes();
        if b.len() < 10 || b[4] != b'-' || b[7] != b'-' {
            return None;
        }
        let day = (parse(s)? + 25_569) * 86_400_000_000;
        let rest = &s[10..];
        if rest.is_empty() {
            return Some(day);
        }
        let t = rest.strip_prefix(' ').or_else(|| rest.strip_prefix('T'))?;
        let mut it = t.splitn(3, ':');
        let h: i64 = it.next()?.parse().ok()?;
        let m: i64 = it.next()?.parse().ok()?;
        let sec: f64 = it.next().unwrap_or("0").parse().ok()?;
        Some(day + (h * 3600 + m * 60) * 1_000_000 + (sec * 1e6).round() as i64)
    }

    /// Microseconds since 1899-12-30 as DuckDB writes a TIMESTAMP: trailing zeros of
    /// the fraction left off.
    pub fn format_micros(micros: i64) -> String {
        let per_day = 86_400_000_000i64;
        let day = micros.div_euclid(per_day) - 25_569;
        let mut rest = micros.rem_euclid(per_day);
        let us = rest % 1_000_000;
        rest /= 1_000_000;
        let (h, m, s) = (rest / 3600, rest / 60 % 60, rest % 60);
        let frac = if us == 0 {
            String::new()
        } else {
            format!(".{us:06}").trim_end_matches('0').to_string()
        };
        format!("{} {h:02}:{m:02}:{s:02}{frac}", format(day))
    }

    pub fn parse(s: &str) -> Option<i64> {
        let mut it = s.get(..10)?.split('-');
        let y = it.next()?.parse().ok()?;
        let m = it.next()?.parse().ok()?;
        let d = it.next()?.parse().ok()?;
        Some(days(y, m, d))
    }

    /// DATE ( y, m, d ): years 0 to 1899 count from 1900, and months and days outside
    /// their range carry over (month 13 is January of the next year, day 0 the last
    /// day of the month before).
    pub fn date(y: i64, m: i64, d: i64) -> i64 {
        let y = if (0..1900).contains(&y) { y + 1900 } else { y };
        let total = y * 12 + m - 1;
        days(total.div_euclid(12), (total.rem_euclid(12) + 1) as u32, 1) + d - 1
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

        #[test]
        fn dates_carry_over_like_dax() {
            assert_eq!(format(date(2024, 13, 1)), "2025-01-01");
            assert_eq!(format(date(2024, 3, 0)), "2024-02-29");
            assert_eq!(format(date(2024, 0, 1)), "2023-12-01");
            assert_eq!(format(date(2024, 1, 366)), "2024-12-31");
            assert_eq!(format(date(24, 1, 1)), "1924-01-01");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_numbers_and_dates() {
        assert_eq!(format_number(1234.5, 0, true, false, ""), "1,235");
        assert_eq!(format_number(-1234.5, 2, true, false, "$"), "-$1,234.50");
        assert_eq!(format_number(0.1234, 1, false, true, ""), "12.3%");
        assert_eq!(format_number(-0.004, 2, false, false, ""), "0.00");
        assert_eq!(
            format_number(999999.999, 2, true, false, ""),
            "1,000,000.00"
        );
        let parts = match charted_dax::format::parse("dddd d mmm yy").unwrap() {
            FormatSpec::Date(p) => p,
            _ => unreachable!(),
        };
        assert_eq!(
            format_date(cal::parse("2024-03-05").unwrap(), &parts),
            "Tuesday 5 Mar 24"
        );
    }

    #[test]
    fn library_functions_follow_power_bi() {
        // As the Power BI Service answered (see library.rs).
        let t = |s: &str| Value::Text(s.into());
        let n = Value::Num;
        let run = |f: ScalarFunc, a: &[Value]| scalar(&f, a).unwrap();
        let lib = |f: LibFn, a: &[Value]| scalar(&ScalarFunc::Lib(f), a);
        assert_eq!(
            run(ScalarFunc::Lib(LibFn::Len), &[Value::Blank]),
            Value::Blank
        );
        assert_eq!(
            run(ScalarFunc::Lib(LibFn::Upper), &[Value::Blank]),
            Value::Blank
        );
        assert_eq!(lib(LibFn::Left, &[t("abc"), Value::Blank]).unwrap(), t(""));
        assert_eq!(lib(LibFn::Right, &[t("abc"), n(1.9)]).unwrap(), t("bc"));
        assert_eq!(
            lib(LibFn::Mid, &[t("abcdef"), n(2.7), n(2.0)]).unwrap(),
            t("cd")
        );
        assert!(lib(LibFn::Left, &[t("abc"), n(-1.0)]).is_err());
        assert!(lib(LibFn::Mid, &[t("abc"), n(0.0), n(2.0)]).is_err());
        assert_eq!(
            lib(LibFn::Mod, &[Value::Blank, n(3.0)]).unwrap(),
            Value::Blank
        );
        assert!(lib(LibFn::Mod, &[n(3.0), Value::Blank]).is_err());
        assert_eq!(lib(LibFn::Mod, &[n(7.0), n(-3.0)]).unwrap(), n(-2.0));
        assert_eq!(
            lib(LibFn::Round, &[n(2.567), Value::Blank]).unwrap(),
            n(3.0)
        );
        let max = |a: Value, b: Value| run(ScalarFunc::Max, &[a, b]);
        let min = |a: Value, b: Value| run(ScalarFunc::Min, &[a, b]);
        assert_eq!(max(Value::Blank, n(-1.0)), Value::Blank);
        assert_eq!(max(Value::Blank, n(1.0)), n(1.0));
        assert_eq!(max(Value::Blank, n(0.0)), n(0.0));
        assert_eq!(min(Value::Blank, n(1.0)), Value::Blank);
        assert_eq!(min(n(0.0), Value::Blank), n(0.0));
        assert_eq!(
            run(ScalarFunc::Lib(LibFn::Trim), &[t("  a   b ")]),
            t("a b")
        );
        assert_eq!(
            run(ScalarFunc::Lib(LibFn::Right), &[t("abc"), Value::Num(5.0)]),
            t("abc")
        );
        assert_eq!(
            run(
                ScalarFunc::Substitute,
                &[t("a-b-c"), t("-"), t("+"), Value::Num(2.0)]
            ),
            t("a-b+c")
        );
        assert_eq!(
            run(ScalarFunc::Lib(LibFn::Int), &[Value::Num(-2.5)]),
            Value::Num(-3.0)
        );
        assert_eq!(
            run(ScalarFunc::Min, &[Value::Num(1.0), Value::Blank]),
            Value::Blank
        );
        assert_eq!(
            run(
                ScalarFunc::DateDiff(Period::Month),
                &[t("2024-03-31"), t("2024-01-01")]
            ),
            Value::Num(-2.0)
        );
    }
}
