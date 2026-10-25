//! Resolves names in a parsed DAX expression against the model and checks that charted
//! can evaluate it. The result (`Bound`) refers to tables, columns and measures by id
//! and makes DAX's structure explicit: table expressions, CALCULATE filter arguments,
//! row contexts. Both the SQL compiler and the reference interpreter consume it.

use charted_model::{Cardinality, DataType, Id, Model, Table};

use crate::parser::{BinaryOp, Expr, ExprKind, UnaryOp};
use crate::{Diagnostic, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AggFunc {
    Sum,
    Average,
    Min,
    Max,
    /// Non-blank values.
    Count,
    CountBlank,
    CountRows,
    DistinctCount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarFunc {
    Divide,
    If,
    Abs,
    Round,
    Coalesce,
    And,
    Or,
    Not,
    IsBlank,
}

/// Facts about the filter context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfoFunc {
    /// The column's only visible value, else the alternate (blank by default).
    SelectedValue,
    HasOneValue,
    /// The column (or any column of the table) is filtered.
    IsFiltered,
    /// The column is a grouping level of the current visual row.
    IsInScope,
}

/// Rows to iterate or filter by.
#[derive(Debug, Clone, PartialEq)]
pub enum TableExpr {
    /// The visible rows of a model table.
    Table(Id),
    /// The distinct visible values of columns of one table (VALUES, DISTINCT).
    Values { table: Id, columns: Vec<Id> },
    /// All rows (or all values of some columns) of a table, ignoring filters.
    All { table: Id, columns: Vec<Id> },
    Filter {
        source: Box<TableExpr>,
        condition: Box<Bound>,
    },
    /// A one-column table of literal values.
    List(Vec<Bound>),
}

impl TableExpr {
    /// The model table whose rows this iterates (none for literal lists).
    pub fn table(&self) -> Option<&Id> {
        match self {
            TableExpr::Table(t)
            | TableExpr::Values { table: t, .. }
            | TableExpr::All { table: t, .. } => Some(t),
            TableExpr::Filter { source, .. } => source.table(),
            TableExpr::List(_) => None,
        }
    }

    /// The columns a row of this table has (`None`: every column of the table).
    pub fn columns(&self) -> Option<&[Id]> {
        match self {
            TableExpr::Table(_) => None,
            TableExpr::Values { columns, .. } | TableExpr::All { columns, .. }
                if !columns.is_empty() =>
            {
                Some(columns)
            }
            TableExpr::Values { .. } | TableExpr::All { .. } => None,
            TableExpr::Filter { source, .. } => source.columns(),
            TableExpr::List(_) => Some(&[]),
        }
    }
}

/// A CALCULATE filter argument.
#[derive(Debug, Clone, PartialEq)]
pub enum FilterArg {
    /// Replaces the filters on the table's columns with its rows. A boolean condition
    /// `T[c] > 5` is `FILTER ( ALL ( T[c] ), T[c] > 5 )`.
    Table(TableExpr),
    /// ALL / REMOVEFILTERS: removes the filters on these columns, or on the table and
    /// every table it reaches (its expanded table) when `columns` is empty.
    RemoveFilters { table: Id, columns: Vec<Id> },
    /// ALLEXCEPT: removes the filters on the expanded table except on `keep`.
    AllExcept { table: Id, keep: Vec<Id> },
    /// ALLSELECTED: removes the visual's grouping on these columns (or on the table),
    /// keeping filters from the filter pane and slicers.
    RemoveGrouping { table: Id, columns: Vec<Id> },
    /// KEEPFILTERS: adds the filter without replacing existing ones.
    Keep(Box<FilterArg>),
    /// USERELATIONSHIP: uses this (inactive) relationship instead of the active one.
    UseRelationship(Id),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Bound {
    Number(f64),
    Text(String),
    Bool(bool),
    Blank,
    /// A column of the current row (inside an iterator, FILTER or a calculated column).
    Column {
        table: Id,
        column: Id,
    },
    /// RELATED: a column of the table the current row relates to.
    Related {
        table: Id,
        column: Id,
    },
    Measure {
        table: Id,
        measure: Id,
    },
    Var(String),
    Let {
        name: String,
        value: Box<Bound>,
        body: Box<Bound>,
    },
    /// An aggregate over the rows of `source` (in the filter context). `arg` is
    /// evaluated per row; COUNTROWS has none.
    Aggregate {
        func: AggFunc,
        source: TableExpr,
        arg: Option<Box<Bound>>,
    },
    Calculate {
        expr: Box<Bound>,
        filters: Vec<FilterArg>,
    },
    Call {
        func: ScalarFunc,
        args: Vec<Bound>,
    },
    Info {
        func: InfoFunc,
        table: Id,
        /// Empty for ISFILTERED on a whole table.
        column: Option<Id>,
        alternate: Option<Box<Bound>>,
    },
    Switch {
        value: Box<Bound>,
        cases: Vec<(Bound, Bound)>,
        default: Option<Box<Bound>>,
    },
    In {
        value: Box<Bound>,
        list: Vec<Bound>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Bound>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Bound>,
        right: Box<Bound>,
    },
}

/// DAX functions charted knows of but cannot evaluate yet, so their message can say so.
pub const KNOWN: &[&str] = &[
    "ADDCOLUMNS",
    "AVERAGEA",
    "CALCULATETABLE",
    "CEILING",
    "CONCATENATE",
    "CONCATENATEX",
    "COUNTAX",
    "CROSSFILTER",
    "DATE",
    "DATEADD",
    "DATEDIFF",
    "DATESBETWEEN",
    "DATESINPERIOD",
    "DATESMTD",
    "DATESQTD",
    "DATESYTD",
    "DATEVALUE",
    "DAY",
    "EARLIER",
    "EDATE",
    "EOMONTH",
    "EXP",
    "FIND",
    "FIRSTDATE",
    "FIRSTNONBLANK",
    "FLOOR",
    "FORMAT",
    "GENERATE",
    "HOUR",
    "IFERROR",
    "INT",
    "ISERROR",
    "LASTDATE",
    "LASTNONBLANK",
    "LEFT",
    "LEN",
    "LN",
    "LOG",
    "LOOKUPVALUE",
    "LOWER",
    "MAXA",
    "MEDIAN",
    "MEDIANX",
    "MID",
    "MINA",
    "MINUTE",
    "MOD",
    "MONTH",
    "NOW",
    "PARALLELPERIOD",
    "PERCENTILE.INC",
    "POWER",
    "PREVIOUSMONTH",
    "PREVIOUSYEAR",
    "PRODUCT",
    "PRODUCTX",
    "QUARTER",
    "RANKX",
    "RELATEDTABLE",
    "REPLACE",
    "RIGHT",
    "ROUNDDOWN",
    "ROUNDUP",
    "SAMEPERIODLASTYEAR",
    "SEARCH",
    "SELECTCOLUMNS",
    "SIGN",
    "SQRT",
    "STDEV.P",
    "STDEV.S",
    "SUBSTITUTE",
    "SUMMARIZE",
    "SUMMARIZECOLUMNS",
    "TODAY",
    "TOPN",
    "TOTALMTD",
    "TOTALQTD",
    "TOTALYTD",
    "TREATAS",
    "TRIM",
    "TRUNC",
    "UPPER",
    "VALUE",
    "WEEKDAY",
    "WEEKNUM",
    "YEAR",
    "YEARFRAC",
];

/// Functions charted evaluates, for docs/dax-support.md.
pub const SUPPORTED: &[&str] = &[
    "ABS",
    "ALL",
    "ALLEXCEPT",
    "ALLSELECTED",
    "AND",
    "AVERAGE",
    "AVERAGEX",
    "BLANK",
    "CALCULATE",
    "COALESCE",
    "COUNT",
    "COUNTA",
    "COUNTBLANK",
    "COUNTROWS",
    "COUNTX",
    "DISTINCT",
    "DISTINCTCOUNT",
    "DIVIDE",
    "FALSE",
    "FILTER",
    "HASONEVALUE",
    "IF",
    "ISBLANK",
    "ISFILTERED",
    "ISINSCOPE",
    "KEEPFILTERS",
    "MAX",
    "MAXX",
    "MIN",
    "MINX",
    "NOT",
    "OR",
    "RELATED",
    "REMOVEFILTERS",
    "ROUND",
    "SELECTEDVALUE",
    "SUM",
    "SUMX",
    "SWITCH",
    "TRUE",
    "USERELATIONSHIP",
    "VALUES",
];

/// Binds `expr`.
pub fn bind(model: &Model, expr: &Expr) -> (Option<Bound>, Vec<Diagnostic>) {
    bind_in(model, expr, None)
}

/// Binds an expression evaluated per row of `row_table` (a calculated column).
pub fn bind_in(
    model: &Model,
    expr: &Expr,
    row_table: Option<&str>,
) -> (Option<Bound>, Vec<Diagnostic>) {
    let mut b = Binder {
        model,
        diags: Vec::new(),
        vars: Vec::new(),
    };
    let rows: Vec<&Table> = row_table.and_then(|t| model.table(t)).into_iter().collect();
    let bound = b.expr(expr, &rows);
    let ok = b.diags.is_empty();
    (bound.filter(|_| ok), b.diags)
}

struct Binder<'a> {
    model: &'a Model,
    diags: Vec<Diagnostic>,
    /// Variables in scope, innermost last.
    vars: Vec<String>,
}

/// Tables whose rows are being iterated, innermost last.
type Rows<'a, 'b> = &'b [&'a Table];

/// Whether `to` is on the one side of a chain of active relationships from `from`.
pub fn reaches(model: &Model, from: &str, to: &str) -> bool {
    let mut seen = vec![from.to_string()];
    let mut i = 0;
    while i < seen.len() {
        let t = seen[i].clone();
        for r in model.relationships.iter().filter(|r| r.active) {
            let next = match (r.from_cardinality, r.to_cardinality) {
                (Cardinality::Many, Cardinality::One) if r.from_table == t => Some(&r.to_table),
                (Cardinality::One, Cardinality::Many) if r.to_table == t => Some(&r.from_table),
                (Cardinality::One, Cardinality::One) if r.from_table == t => Some(&r.to_table),
                (Cardinality::One, Cardinality::One) if r.to_table == t => Some(&r.from_table),
                _ => None,
            };
            if let Some(n) = next
                && !seen.contains(n)
            {
                if n == to {
                    return true;
                }
                seen.push(n.clone());
            }
        }
        i += 1;
    }
    false
}

impl<'a> Binder<'a> {
    fn err(&mut self, span: Span, message: impl Into<String>) -> Option<Bound> {
        self.diags.push(Diagnostic::error(span, message.into()));
        None
    }

    fn table(&mut self, name: &str, span: Span) -> Option<&'a Table> {
        let t = self.model.table_by_name(name);
        if t.is_none() {
            self.err(span, format!("there's no table named '{name}'"));
        }
        t
    }

    fn measure_named(&self, name: &str) -> Option<(&'a Table, &'a charted_model::Measure)> {
        self.model.tables.iter().find_map(|t| {
            t.measures
                .iter()
                .find(|m| m.name.eq_ignore_ascii_case(name))
                .map(|m| (t, m))
        })
    }

    fn expr(&mut self, e: &Expr, rows: Rows<'a, '_>) -> Option<Bound> {
        match &e.kind {
            ExprKind::Number(n) => Some(Bound::Number(*n)),
            ExprKind::Text(s) => Some(Bound::Text(s.clone())),
            ExprKind::Bool(v) => Some(Bound::Bool(*v)),
            ExprKind::Error => None,
            ExprKind::Table(name) => {
                if let Some(v) = self
                    .vars
                    .iter()
                    .rev()
                    .find(|v| v.eq_ignore_ascii_case(name))
                {
                    return Some(Bound::Var(v.clone()));
                }
                self.table(name, e.span)?;
                self.err(
                    e.span,
                    format!("'{name}' is a table; use it inside a function such as COUNTROWS"),
                )
            }
            ExprKind::List(_) => self.err(e.span, "a list of values can only be used with IN"),
            ExprKind::Ref { table, name } => self.reference(table.as_deref(), name, e.span, rows),
            ExprKind::Var {
                name, value, body, ..
            } => {
                let v = self.expr(value, rows);
                self.vars.push(name.clone());
                let b = self.expr(body, rows);
                self.vars.pop();
                Some(Bound::Let {
                    name: name.clone(),
                    value: Box::new(v?),
                    body: Box::new(b?),
                })
            }
            ExprKind::Unary { op, expr } => {
                let inner = self.expr(expr, rows)?;
                Some(Bound::Unary {
                    op: *op,
                    expr: Box::new(inner),
                })
            }
            ExprKind::Binary {
                op: BinaryOp::In,
                left,
                right,
            } => {
                let v = self.expr(left, rows);
                let ExprKind::List(items) = &right.kind else {
                    return self.err(
                        right.span,
                        "IN needs a list of values, such as { \"Red\", \"Blue\" }",
                    );
                };
                let list: Vec<Option<Bound>> = items.iter().map(|i| self.expr(i, rows)).collect();
                Some(Bound::In {
                    value: Box::new(v?),
                    list: list.into_iter().collect::<Option<Vec<_>>>()?,
                })
            }
            ExprKind::Binary { op, left, right } => {
                let l = self.expr(left, rows);
                let r = self.expr(right, rows);
                Some(Bound::Binary {
                    op: *op,
                    left: Box::new(l?),
                    right: Box::new(r?),
                })
            }
            ExprKind::Call { name, args } => self.call(name, args, e.span, rows),
        }
    }

    fn reference(
        &mut self,
        table: Option<&str>,
        name: &str,
        span: Span,
        rows: Rows<'a, '_>,
    ) -> Option<Bound> {
        let (t, c) = match table {
            Some(tn) => {
                let t = self.table(tn, span)?;
                if let Some(m) = t
                    .measures
                    .iter()
                    .find(|m| m.name.eq_ignore_ascii_case(name))
                {
                    return Some(Bound::Measure {
                        table: t.id.clone(),
                        measure: m.id.clone(),
                    });
                }
                match t.column_by_name(name) {
                    Some(c) => (t, c),
                    None => {
                        return self.err(
                            span,
                            format!("table '{}' has no column or measure named [{name}]", t.name),
                        );
                    }
                }
            }
            None => {
                if let Some((t, m)) = self.measure_named(name) {
                    return Some(Bound::Measure {
                        table: t.id.clone(),
                        measure: m.id.clone(),
                    });
                }
                match rows
                    .iter()
                    .rev()
                    .find_map(|t| t.column_by_name(name).map(|c| (*t, c)))
                {
                    Some(tc) => tc,
                    None => return self.err(span, format!("there's no measure named [{name}]")),
                }
            }
        };
        if rows.iter().any(|r| r.id == t.id) {
            return Some(Bound::Column {
                table: t.id.clone(),
                column: c.id.clone(),
            });
        }
        if let Some(r) = rows.last() {
            return self.err(
                span,
                format!(
                    "'{}'[{}] isn't a column of '{}'; use RELATED ( '{}'[{}] ) to look it up",
                    t.name, c.name, r.name, t.name, c.name
                ),
            );
        }
        self.err(
            span,
            format!(
                "a single value for column '{}'[{}] can't be determined here; wrap it in an aggregation such as SUM",
                t.name, c.name
            ),
        )
    }

    /// A `Table[Column]` argument, by name, without row-context checks.
    fn column_arg(&mut self, e: &Expr) -> Option<(&'a Table, &'a charted_model::Column)> {
        let ExprKind::Ref { table, name } = &e.kind else {
            return self
                .err(e.span, "this needs a column, such as Sales[Amount]")
                .and(None);
        };
        let Some(tn) = table else {
            return self
                .err(
                    e.span,
                    format!("name the table of [{name}], as in Sales[{name}]"),
                )
                .and(None);
        };
        let t = self.table(tn, e.span)?;
        match t.column_by_name(name) {
            Some(c) => Some((t, c)),
            None => self
                .err(
                    e.span,
                    format!("table '{}' has no column named [{name}]", t.name),
                )
                .and(None),
        }
    }

    fn arity(&mut self, name: &str, args: &[Expr], span: Span, min: usize, max: usize) -> bool {
        if args.len() < min || args.len() > max {
            let want = if min == max {
                format!("{min}")
            } else if max == usize::MAX {
                format!("at least {min}")
            } else {
                format!("{min} to {max}")
            };
            self.err(
                span,
                format!("{name} takes {want} argument(s), not {}", args.len()),
            );
            return false;
        }
        true
    }

    /// A table expression: a table name, VALUES, DISTINCT, ALL, FILTER or a list.
    fn table_expr(&mut self, e: &Expr, rows: Rows<'a, '_>) -> Option<TableExpr> {
        match &e.kind {
            ExprKind::Table(name) => Some(TableExpr::Table(self.table(name, e.span)?.id.clone())),
            ExprKind::List(items) => {
                let list: Vec<Option<Bound>> = items.iter().map(|i| self.expr(i, rows)).collect();
                Some(TableExpr::List(list.into_iter().collect::<Option<Vec<_>>>()?))
            }
            ExprKind::Call { name, args } => {
                let upper = name.to_ascii_uppercase();
                match upper.as_str() {
                    "VALUES" | "DISTINCT" => {
                        if !self.arity(&upper, args, e.span, 1, 1) {
                            return None;
                        }
                        if let ExprKind::Table(tn) = &args[0].kind {
                            return Some(TableExpr::Table(self.table(tn, args[0].span)?.id.clone()));
                        }
                        let (t, c) = self.column_arg(&args[0])?;
                        Some(TableExpr::Values {
                            table: t.id.clone(),
                            columns: vec![c.id.clone()],
                        })
                    }
                    "ALL" | "REMOVEFILTERS" | "ALLSELECTED" => {
                        let (table, columns) = self.all_args(&upper, args, e.span)?;
                        Some(TableExpr::All { table, columns })
                    }
                    "FILTER" => {
                        if !self.arity(&upper, args, e.span, 2, 2) {
                            return None;
                        }
                        let source = self.table_expr(&args[0], rows)?;
                        let Some(t) = source.table().and_then(|t| self.model.table(t)) else {
                            return self.err(args[0].span, "FILTER over a list of values isn't supported yet").and(None);
                        };
                        let mut inner: Vec<&Table> = rows.to_vec();
                        inner.push(t);
                        let condition = self.expr(&args[1], &inner)?;
                        Some(TableExpr::Filter {
                            source: Box::new(source),
                            condition: Box::new(condition),
                        })
                    }
                    _ => self
                        .err(
                            e.span,
                            format!("{upper} doesn't return a table charted can use here (try a table name, VALUES, ALL or FILTER)"),
                        )
                        .and(None),
                }
            }
            _ => self
                .err(e.span, "this needs a table, such as Sales, VALUES ( Product[Category] ) or FILTER ( … )")
                .and(None),
        }
    }

    /// ALL/REMOVEFILTERS/ALLSELECTED arguments: a table, or columns of one table.
    fn all_args(&mut self, name: &str, args: &[Expr], span: Span) -> Option<(Id, Vec<Id>)> {
        if args.is_empty() {
            return self
                .err(
                    span,
                    format!(
                        "{name} without arguments isn't supported yet; name a table or columns"
                    ),
                )
                .and(None);
        }
        if let [
            Expr {
                kind: ExprKind::Table(tn),
                span: s,
            },
        ] = args
        {
            return Some((self.table(tn, *s)?.id.clone(), Vec::new()));
        }
        let mut table: Option<Id> = None;
        let mut columns = Vec::new();
        for a in args {
            let (t, c) = self.column_arg(a)?;
            if table.as_ref().is_some_and(|x| x != &t.id) {
                return self
                    .err(
                        a.span,
                        format!("all columns given to {name} must belong to one table"),
                    )
                    .and(None);
            }
            table = Some(t.id.clone());
            columns.push(c.id.clone());
        }
        Some((table?, columns))
    }

    /// A CALCULATE filter argument.
    fn filter_arg(&mut self, e: &Expr, rows: Rows<'a, '_>) -> Option<FilterArg> {
        if let ExprKind::Call { name, args } = &e.kind {
            let upper = name.to_ascii_uppercase();
            match upper.as_str() {
                "ALL" | "REMOVEFILTERS" => {
                    let (table, columns) = self.all_args(&upper, args, e.span)?;
                    return Some(FilterArg::RemoveFilters { table, columns });
                }
                "ALLSELECTED" => {
                    let (table, columns) = self.all_args(&upper, args, e.span)?;
                    return Some(FilterArg::RemoveGrouping { table, columns });
                }
                "ALLEXCEPT" => {
                    if !self.arity(&upper, args, e.span, 2, usize::MAX) {
                        return None;
                    }
                    let ExprKind::Table(tn) = &args[0].kind else {
                        return self
                            .err(args[0].span, "ALLEXCEPT starts with a table name")
                            .and(None);
                    };
                    let t = self.table(tn, args[0].span)?;
                    let mut keep = Vec::new();
                    for a in &args[1..] {
                        keep.push(self.column_arg(a)?.1.id.clone());
                    }
                    return Some(FilterArg::AllExcept {
                        table: t.id.clone(),
                        keep,
                    });
                }
                "KEEPFILTERS" => {
                    if !self.arity(&upper, args, e.span, 1, 1) {
                        return None;
                    }
                    return Some(FilterArg::Keep(Box::new(self.filter_arg(&args[0], rows)?)));
                }
                "USERELATIONSHIP" => {
                    if !self.arity(&upper, args, e.span, 2, 2) {
                        return None;
                    }
                    let (ta, ca) = self.column_arg(&args[0])?;
                    let (tb, cb) = self.column_arg(&args[1])?;
                    let found = self.model.relationships.iter().find(|r| {
                        (r.from_column == ca.id && r.to_column == cb.id)
                            || (r.from_column == cb.id && r.to_column == ca.id)
                    });
                    return match found {
                        Some(r) => Some(FilterArg::UseRelationship(r.id.clone())),
                        None => self
                            .err(
                                e.span,
                                format!(
                                    "there's no relationship between '{}'[{}] and '{}'[{}]",
                                    ta.name, ca.name, tb.name, cb.name
                                ),
                            )
                            .and(None),
                    };
                }
                "FILTER" | "VALUES" | "DISTINCT" => {
                    return Some(FilterArg::Table(self.table_expr(e, rows)?));
                }
                _ => {}
            }
        }
        if let ExprKind::Table(name) = &e.kind {
            return Some(FilterArg::Table(TableExpr::Table(
                self.table(name, e.span)?.id.clone(),
            )));
        }
        // A boolean condition over columns of one table: FILTER ( ALL ( columns ), condition ).
        let mut cols: Vec<(Id, Id)> = Vec::new();
        collect_refs(self.model, e, &mut cols);
        let Some((table, _)) = cols.first().cloned() else {
            return self
                .err(
                    e.span,
                    "a filter condition needs a column, such as Product[Color] = \"Red\"",
                )
                .and(None);
        };
        if cols.iter().any(|(t, _)| *t != table) {
            return self
                .err(
                    e.span,
                    "a filter condition can only use columns of one table; use FILTER for more",
                )
                .and(None);
        }
        let t = self.model.table(&table)?;
        let mut inner: Vec<&Table> = rows.to_vec();
        inner.push(t);
        let condition = self.expr(e, &inner)?;
        let mut columns: Vec<Id> = Vec::new();
        for (_, c) in cols {
            if !columns.contains(&c) {
                columns.push(c);
            }
        }
        Some(FilterArg::Table(TableExpr::Filter {
            source: Box::new(TableExpr::All { table, columns }),
            condition: Box::new(condition),
        }))
    }

    fn call(&mut self, name: &str, args: &[Expr], span: Span, rows: Rows<'a, '_>) -> Option<Bound> {
        let upper = name.to_ascii_uppercase();
        let simple_agg = match upper.as_str() {
            "SUM" => Some(AggFunc::Sum),
            "AVERAGE" => Some(AggFunc::Average),
            "MIN" if args.len() == 1 => Some(AggFunc::Min),
            "MAX" if args.len() == 1 => Some(AggFunc::Max),
            "COUNT" | "COUNTA" => Some(AggFunc::Count),
            "COUNTBLANK" => Some(AggFunc::CountBlank),
            "DISTINCTCOUNT" => Some(AggFunc::DistinctCount),
            _ => None,
        };
        if let Some(func) = simple_agg {
            if !self.arity(&upper, args, span, 1, 1) {
                return None;
            }
            let (t, col) = self.column_arg(&args[0])?;
            if matches!(func, AggFunc::Sum | AggFunc::Average) && !col.data_type.is_numeric() {
                return self.err(
                    args[0].span,
                    format!(
                        "{upper} can't work with {} values; '{}'[{}] isn't numeric",
                        type_name(col.data_type),
                        t.name,
                        col.name
                    ),
                );
            }
            return Some(Bound::Aggregate {
                func,
                source: TableExpr::Table(t.id.clone()),
                arg: Some(Box::new(Bound::Column {
                    table: t.id.clone(),
                    column: col.id.clone(),
                })),
            });
        }
        let iterator = match upper.as_str() {
            "SUMX" => Some(AggFunc::Sum),
            "AVERAGEX" => Some(AggFunc::Average),
            "MINX" => Some(AggFunc::Min),
            "MAXX" => Some(AggFunc::Max),
            "COUNTX" => Some(AggFunc::Count),
            _ => None,
        };
        if let Some(func) = iterator {
            if !self.arity(&upper, args, span, 2, 2) {
                return None;
            }
            let source = self.table_expr(&args[0], rows)?;
            let Some(t) = source.table().and_then(|t| self.model.table(t)) else {
                return self.err(
                    args[0].span,
                    format!("{upper} over a list of values isn't supported yet"),
                );
            };
            let mut inner: Vec<&Table> = rows.to_vec();
            inner.push(t);
            let arg = self.expr(&args[1], &inner)?;
            return Some(Bound::Aggregate {
                func,
                source,
                arg: Some(Box::new(arg)),
            });
        }
        match upper.as_str() {
            "COUNTROWS" => {
                if !self.arity(&upper, args, span, 1, 1) {
                    return None;
                }
                let source = self.table_expr(&args[0], rows)?;
                if source.table().is_none() {
                    return self.err(args[0].span, "COUNTROWS of a list of values isn't supported yet");
                }
                Some(Bound::Aggregate {
                    func: AggFunc::CountRows,
                    source,
                    arg: None,
                })
            }
            "CALCULATE" => {
                if !self.arity(&upper, args, span, 1, usize::MAX) {
                    return None;
                }
                let expr = self.expr(&args[0], rows);
                let filters: Vec<Option<FilterArg>> = args[1..].iter().map(|a| self.filter_arg(a, rows)).collect();
                Some(Bound::Calculate {
                    expr: Box::new(expr?),
                    filters: filters.into_iter().collect::<Option<Vec<_>>>()?,
                })
            }
            "RELATED" => {
                if !self.arity(&upper, args, span, 1, 1) {
                    return None;
                }
                let (t, c) = self.column_arg(&args[0])?;
                let Some(r) = rows.last() else {
                    return self.err(span, "RELATED needs a row context (use it inside SUMX, FILTER or a calculated column)");
                };
                if r.id != t.id && !reaches(self.model, &r.id, &t.id) {
                    return self.err(
                        span,
                        format!("'{}' isn't related to '{}' through active relationships on its one side", t.name, r.name),
                    );
                }
                Some(Bound::Related {
                    table: t.id.clone(),
                    column: c.id.clone(),
                })
            }
            "SELECTEDVALUE" | "HASONEVALUE" | "ISINSCOPE" | "ISFILTERED" => {
                let (min, max) = if upper == "SELECTEDVALUE" { (1, 2) } else { (1, 1) };
                if !self.arity(&upper, args, span, min, max) {
                    return None;
                }
                let func = match upper.as_str() {
                    "SELECTEDVALUE" => InfoFunc::SelectedValue,
                    "HASONEVALUE" => InfoFunc::HasOneValue,
                    "ISINSCOPE" => InfoFunc::IsInScope,
                    _ => InfoFunc::IsFiltered,
                };
                if func == InfoFunc::IsFiltered
                    && let ExprKind::Table(tn) = &args[0].kind
                {
                    let t = self.table(tn, args[0].span)?;
                    return Some(Bound::Info {
                        func,
                        table: t.id.clone(),
                        column: None,
                        alternate: None,
                    });
                }
                let (t, c) = self.column_arg(&args[0])?;
                let alternate = match args.get(1) {
                    Some(a) => Some(Box::new(self.expr(a, rows)?)),
                    None => None,
                };
                Some(Bound::Info {
                    func,
                    table: t.id.clone(),
                    column: Some(c.id.clone()),
                    alternate,
                })
            }
            "SWITCH" => {
                if !self.arity(&upper, args, span, 3, usize::MAX) {
                    return None;
                }
                let value = self.expr(&args[0], rows);
                let mut cases = Vec::new();
                let mut i = 1;
                while i + 1 < args.len() {
                    let w = self.expr(&args[i], rows);
                    let then = self.expr(&args[i + 1], rows);
                    cases.push((w?, then?));
                    i += 2;
                }
                let default = match args.get(i) {
                    Some(d) => Some(Box::new(self.expr(d, rows)?)),
                    None => None,
                };
                Some(Bound::Switch {
                    value: Box::new(value?),
                    cases,
                    default,
                })
            }
            "BLANK" => self.arity(&upper, args, span, 0, 0).then_some(Bound::Blank),
            "TRUE" | "FALSE" => self.arity(&upper, args, span, 0, 0).then_some(Bound::Bool(upper == "TRUE")),
            "VALUES" | "DISTINCT" | "ALL" | "FILTER" | "REMOVEFILTERS" | "ALLEXCEPT" | "ALLSELECTED" | "KEEPFILTERS"
            | "USERELATIONSHIP" => self.err(
                span,
                format!("{upper} returns a table; use it inside CALCULATE, an iterator such as SUMX, or COUNTROWS"),
            ),
            _ => {
                let (func, min, max) = match upper.as_str() {
                    "DIVIDE" => (ScalarFunc::Divide, 2, 3),
                    "IF" => (ScalarFunc::If, 2, 3),
                    "ABS" => (ScalarFunc::Abs, 1, 1),
                    "ROUND" => (ScalarFunc::Round, 2, 2),
                    "COALESCE" => (ScalarFunc::Coalesce, 2, usize::MAX),
                    "AND" => (ScalarFunc::And, 2, 2),
                    "OR" => (ScalarFunc::Or, 2, 2),
                    "NOT" => (ScalarFunc::Not, 1, 1),
                    "ISBLANK" => (ScalarFunc::IsBlank, 1, 1),
                    "MIN" | "MAX" => {
                        return self.err(span, format!("{upper} of two values isn't supported yet"));
                    }
                    _ if KNOWN.contains(&upper.as_str()) => {
                        return self.err(span, format!("{upper} isn't supported by charted yet (see docs/dax-support.md)"));
                    }
                    _ => return self.err(span, format!("{name} isn't a DAX function")),
                };
                if !self.arity(&upper, args, span, min, max) {
                    return None;
                }
                let bound: Vec<Option<Bound>> = args.iter().map(|a| self.expr(a, rows)).collect();
                Some(Bound::Call {
                    func,
                    args: bound.into_iter().collect::<Option<Vec<_>>>()?,
                })
            }
        }
    }
}

/// Column references (`Table[Column]`, by id) that appear in an expression.
fn collect_refs(model: &Model, e: &Expr, out: &mut Vec<(Id, Id)>) {
    match &e.kind {
        ExprKind::Ref {
            table: Some(tn),
            name,
        } => {
            if let Some(t) = model.table_by_name(tn)
                && let Some(c) = t.column_by_name(name)
            {
                out.push((t.id.clone(), c.id.clone()));
            }
        }
        ExprKind::Call { args, .. } => args.iter().for_each(|a| collect_refs(model, a, out)),
        ExprKind::List(items) => items.iter().for_each(|a| collect_refs(model, a, out)),
        ExprKind::Unary { expr, .. } => collect_refs(model, expr, out),
        ExprKind::Binary { left, right, .. } => {
            collect_refs(model, left, out);
            collect_refs(model, right, out);
        }
        ExprKind::Var { value, body, .. } => {
            collect_refs(model, value, out);
            collect_refs(model, body, out);
        }
        _ => {}
    }
}

fn type_name(t: DataType) -> &'static str {
    match t {
        DataType::String => "text",
        DataType::Boolean => "true/false",
        DataType::Date | DataType::DateTime | DataType::DateTimeTz | DataType::Time => "date",
        _ => "these",
    }
}

/// Measures that `bound` refers to directly.
pub fn measure_refs(bound: &Bound, out: &mut Vec<Id>) {
    walk(bound, &mut |b| {
        if let Bound::Measure { measure, .. } = b {
            out.push(measure.clone());
        }
    });
}

/// Visits every node of a bound expression, including inside table expressions and
/// filter arguments.
pub fn walk(b: &Bound, f: &mut dyn FnMut(&Bound)) {
    f(b);
    match b {
        Bound::Let { value, body, .. } => {
            walk(value, f);
            walk(body, f);
        }
        Bound::Aggregate { source, arg, .. } => {
            walk_table(source, f);
            if let Some(a) = arg {
                walk(a, f);
            }
        }
        Bound::Calculate { expr, filters } => {
            walk(expr, f);
            for fa in filters {
                walk_filter(fa, f);
            }
        }
        Bound::Call { args, .. } => args.iter().for_each(|a| walk(a, f)),
        Bound::Info {
            alternate: Some(a), ..
        } => walk(a, f),
        Bound::Switch {
            value,
            cases,
            default,
        } => {
            walk(value, f);
            for (w, t) in cases {
                walk(w, f);
                walk(t, f);
            }
            if let Some(d) = default {
                walk(d, f);
            }
        }
        Bound::In { value, list } => {
            walk(value, f);
            list.iter().for_each(|a| walk(a, f));
        }
        Bound::Unary { expr, .. } => walk(expr, f),
        Bound::Binary { left, right, .. } => {
            walk(left, f);
            walk(right, f);
        }
        _ => {}
    }
}

fn walk_table(t: &TableExpr, f: &mut dyn FnMut(&Bound)) {
    match t {
        TableExpr::Filter { source, condition } => {
            walk_table(source, f);
            walk(condition, f);
        }
        TableExpr::List(items) => items.iter().for_each(|a| walk(a, f)),
        _ => {}
    }
}

fn walk_filter(fa: &FilterArg, f: &mut dyn FnMut(&Bound)) {
    match fa {
        FilterArg::Table(t) => walk_table(t, f),
        FilterArg::Keep(inner) => walk_filter(inner, f),
        _ => {}
    }
}
