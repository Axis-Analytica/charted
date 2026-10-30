//! Resolves names in a parsed DAX expression against the model and checks that charted
//! can evaluate it. The result (`Bound`) refers to tables, columns and measures by id
//! and makes DAX's structure explicit: table expressions, CALCULATE filter arguments,
//! row contexts. Both the SQL compiler and the reference interpreter consume it.

use charted_model::{Cardinality, DataType, Id, Model, Table};

use crate::format::FormatSpec;
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

#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// YEAR, MONTH, DAY and QUARTER of a date.
    Year,
    Month,
    Day,
    Quarter,
    /// INT: rounds down, towards negative infinity.
    Int,
    /// MIN and MAX of two values: blank counts as zero unless both are blank.
    Min,
    Max,
    /// Text functions. A blank argument counts as empty text.
    Upper,
    Lower,
    Len,
    Trim,
    Left,
    Right,
    Mid,
    /// SUBSTITUTE ( text, old, new, [instance] ).
    Substitute,
    /// FORMAT ( value, "literal format string" ), the format parsed by the binder.
    Format(FormatSpec),
    /// DATE ( year, month, day ): months and days past their range carry over.
    Date,
    /// EOMONTH ( date, months ): the last day of the month `months` away.
    EoMonth,
    /// TODAY: the current date.
    Today,
    /// MOD: the remainder, with the divisor's sign.
    Mod,
    /// WEEKDAY: the day of the week (return type 1: Sunday 1…, 2: Monday 1…, 3: Monday 0…).
    Weekday,
    /// EDATE ( date, months ): the same day `months` away (clamped to the month's end).
    EDate,
    /// DATEDIFF ( date1, date2, unit ): the unit boundaries crossed from date1 to date2.
    DateDiff(Period),
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
    /// LOOKUPVALUE's result: the column's only visible value, the alternate when there
    /// is none, an error when there are several.
    Lookup,
}

/// A calendar unit of the time-intelligence functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    Day,
    Month,
    Quarter,
    Year,
}

impl Period {
    pub fn sql(self) -> &'static str {
        match self {
            Period::Day => "day",
            Period::Month => "month",
            Period::Quarter => "quarter",
            Period::Year => "year",
        }
    }
}

/// How a time-intelligence function turns the visible dates into other dates.
#[derive(Debug, Clone, PartialEq)]
pub enum DateOp {
    /// DATEADD, SAMEPERIODLASTYEAR: each date moved by `n` units; a month's last day
    /// also brings the rest of the target month (so whole months stay whole).
    Shift(i32, Period),
    /// PARALLELPERIOD: the whole periods from the first to the last date, moved by `n`.
    Parallel(i32, Period),
    /// PREVIOUSDAY/MONTH/QUARTER/YEAR: the whole period before the first date.
    Previous(Period),
    /// NEXTDAY/MONTH/QUARTER/YEAR: the whole period after the last date.
    Next(Period),
    /// DATESMTD/QTD/YTD: from the start of the last date's period to the last date. A
    /// year can end on another day (month, day), as in "6/30".
    ToDate(Period, Option<(u32, u32)>),
    /// DATESINPERIOD: `n` units starting at the anchor date (backwards when `n` is
    /// negative, ending at the anchor). Going back from a month's last day lands on the
    /// target month's last day, so "the last 12 months" from a month end is 12 whole
    /// months.
    InPeriod(Anchor, i32, Period),
    /// DATESBETWEEN: the dates from the first anchor to the second, inclusive.
    Between(Anchor, Anchor),
}

/// A date argument of DATESINPERIOD and DATESBETWEEN, evaluated in the outer filter
/// context like the other time-intelligence inputs.
#[derive(Debug, Clone, PartialEq)]
pub enum Anchor {
    /// FIRSTDATE or MIN of the date column: the first visible date.
    First,
    /// LASTDATE or MAX of the date column: the last visible date.
    Last,
    /// DATE ( year, month, day ) with constant parts (they carry over as in DATE).
    Date(i32, i32, i32),
    /// BLANK (): no bound (DATESBETWEEN only).
    Open,
    /// Any other date expression (`EOMONTH ( MAX ( 'Date'[Date] ), -1 )`, `TODAY ()`),
    /// evaluated in the outer filter context.
    Expr(Box<Bound>),
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
    /// ALLSELECTED as a table: the visible rows (or values of some columns) without the
    /// visual's grouping and iterators' rows, keeping slicer, page and report filters.
    AllSelected { table: Id, columns: Vec<Id> },
    Filter {
        source: Box<TableExpr>,
        condition: Box<Bound>,
    },
    /// A one-column table of literal values.
    List(Vec<Bound>),
    /// Dates of a date column computed from the visible ones (time intelligence).
    Dates { table: Id, column: Id, op: DateOp },
    /// CALCULATETABLE (and RELATEDTABLE, with no filters): the rows of `source` in the
    /// context changed by `filters` (the current rows transitioned).
    CalculateTable {
        source: Box<TableExpr>,
        filters: Vec<FilterArg>,
    },
    /// ADDCOLUMNS: the rows of `source` with columns computed per row.
    AddColumns {
        source: Box<TableExpr>,
        cols: Vec<(String, Bound)>,
    },
    /// SELECTCOLUMNS: only the columns computed per row of `source`.
    SelectColumns {
        source: Box<TableExpr>,
        cols: Vec<(String, Bound)>,
    },
    /// SUMMARIZE ( table, columns… ): the distinct combinations of the group columns
    /// (of the table and the tables it reaches) in the table's visible rows.
    Summarize {
        source: Box<TableExpr>,
        group: Vec<(Id, Id)>,
    },
    /// SUMMARIZECOLUMNS: the combinations of the group columns' visible values (under
    /// `filters`), with columns computed per combination; combinations where every
    /// computed column is blank are left out.
    SummarizeColumns {
        group: Vec<(Id, Id)>,
        filters: Vec<FilterArg>,
        cols: Vec<(String, Bound)>,
    },
    /// CALENDAR: one row per date from `start` to `end`, in a column `Date`.
    Calendar { start: Box<Bound>, end: Box<Bound> },
    /// GENERATESERIES: `start`, `start + step`… up to `end`, in a column `Value`.
    GenerateSeries {
        start: Box<Bound>,
        end: Box<Bound>,
        step: Box<Bound>,
    },
    /// TOPN: the rows of `source` ranked within the first `n` by the order expressions
    /// (evaluated per row; `true` ascending), ties at the cut kept.
    TopN {
        n: Box<Bound>,
        source: Box<TableExpr>,
        order: Vec<(Bound, bool)>,
    },
}

impl TableExpr {
    /// The model table whose rows this iterates (none for literal lists).
    pub fn table(&self) -> Option<&Id> {
        match self {
            TableExpr::Table(t)
            | TableExpr::Values { table: t, .. }
            | TableExpr::All { table: t, .. }
            | TableExpr::AllSelected { table: t, .. }
            | TableExpr::Dates { table: t, .. } => Some(t),
            TableExpr::Filter { source, .. }
            | TableExpr::TopN { source, .. }
            | TableExpr::CalculateTable { source, .. }
            | TableExpr::AddColumns { source, .. }
            | TableExpr::SelectColumns { source, .. }
            | TableExpr::Summarize { source, .. } => source.table(),
            TableExpr::SummarizeColumns { group, .. } => group.first().map(|(t, _)| t),
            TableExpr::List(_) | TableExpr::Calendar { .. } | TableExpr::GenerateSeries { .. } => {
                None
            }
        }
    }

    /// The columns a row of this table has (`None`: every column of the table).
    pub fn columns(&self) -> Option<&[Id]> {
        match self {
            TableExpr::Table(_) => None,
            TableExpr::Values { columns, .. }
            | TableExpr::All { columns, .. }
            | TableExpr::AllSelected { columns, .. }
                if !columns.is_empty() =>
            {
                Some(columns)
            }
            TableExpr::Values { .. } | TableExpr::All { .. } | TableExpr::AllSelected { .. } => {
                None
            }
            TableExpr::Dates { column, .. } => Some(std::slice::from_ref(column)),
            TableExpr::Filter { source, .. }
            | TableExpr::TopN { source, .. }
            | TableExpr::CalculateTable { source, .. }
            | TableExpr::AddColumns { source, .. } => source.columns(),
            TableExpr::SelectColumns { .. }
            | TableExpr::Summarize { .. }
            | TableExpr::SummarizeColumns { .. }
            | TableExpr::List(_)
            | TableExpr::Calendar { .. }
            | TableExpr::GenerateSeries { .. } => Some(&[]),
        }
    }
}

/// The key under which a row holds an added column (`[Name]`): not a model id.
pub fn ext_key(name: &str) -> Id {
    format!("\u{0}{}", name.to_lowercase())
}

impl TableExpr {
    /// The columns of the table's rows, in order: each one's key (a model column's id,
    /// or `ext_key` of an added column) and its name.
    pub fn shape(&self, model: &Model) -> Vec<(Id, String)> {
        let model_cols = |ids: &[Id]| -> Vec<(Id, String)> {
            ids.iter()
                .map(|id| {
                    let name = model
                        .tables
                        .iter()
                        .find_map(|t| t.column(id))
                        .map(|c| c.name.clone())
                        .unwrap_or_default();
                    (id.clone(), name)
                })
                .collect()
        };
        let all = |t: &Id| -> Vec<Id> {
            model
                .table(t)
                .map(|t| t.columns.iter().map(|c| c.id.clone()).collect())
                .unwrap_or_default()
        };
        let ext = |cols: &[(String, Bound)]| -> Vec<(Id, String)> {
            cols.iter().map(|(n, _)| (ext_key(n), n.clone())).collect()
        };
        match self {
            TableExpr::Table(t) => model_cols(&all(t)),
            TableExpr::Values { table, columns }
            | TableExpr::All { table, columns }
            | TableExpr::AllSelected { table, columns } => {
                if columns.is_empty() {
                    model_cols(&all(table))
                } else {
                    model_cols(columns)
                }
            }
            TableExpr::Dates { column, .. } => model_cols(std::slice::from_ref(column)),
            TableExpr::Filter { source, .. }
            | TableExpr::TopN { source, .. }
            | TableExpr::CalculateTable { source, .. } => source.shape(model),
            TableExpr::AddColumns { source, cols } => {
                let mut s = source.shape(model);
                s.extend(ext(cols));
                s
            }
            TableExpr::SelectColumns { cols, .. } => ext(cols),
            TableExpr::Summarize { group, .. } => {
                model_cols(&group.iter().map(|(_, c)| c.clone()).collect::<Vec<_>>())
            }
            TableExpr::SummarizeColumns { group, cols, .. } => {
                let mut s = model_cols(&group.iter().map(|(_, c)| c.clone()).collect::<Vec<_>>());
                s.extend(ext(cols));
                s
            }
            TableExpr::Calendar { .. } => vec![(ext_key("Date"), "Date".into())],
            TableExpr::GenerateSeries { .. } => vec![(ext_key("Value"), "Value".into())],
            TableExpr::List(_) => Vec::new(),
        }
    }

    /// Columns added to the rows (`[Name]` inside expressions over them).
    pub fn ext_names(&self) -> Vec<String> {
        match self {
            TableExpr::Filter { source, .. }
            | TableExpr::TopN { source, .. }
            | TableExpr::CalculateTable { source, .. } => source.ext_names(),
            TableExpr::AddColumns { source, cols } => {
                let mut s = source.ext_names();
                s.extend(cols.iter().map(|(n, _)| n.clone()));
                s
            }
            TableExpr::SelectColumns { cols, .. } | TableExpr::SummarizeColumns { cols, .. } => {
                cols.iter().map(|(n, _)| n.clone()).collect()
            }
            TableExpr::Calendar { .. } => vec!["Date".into()],
            TableExpr::GenerateSeries { .. } => vec!["Value".into()],
            _ => Vec::new(),
        }
    }

    /// Model columns of other tables the rows carry (SUMMARIZE's group columns).
    pub fn carried(&self) -> Vec<Id> {
        match self {
            TableExpr::Filter { source, .. }
            | TableExpr::TopN { source, .. }
            | TableExpr::CalculateTable { source, .. }
            | TableExpr::AddColumns { source, .. } => source.carried(),
            TableExpr::Summarize { group, .. } | TableExpr::SummarizeColumns { group, .. } => {
                group.iter().map(|(_, c)| c.clone()).collect()
            }
            _ => Vec::new(),
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
    /// REMOVEFILTERS () or ALL () without arguments: removes every filter in the model.
    RemoveAll,
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
    /// `[Name]`, a column ADDCOLUMNS, SELECTCOLUMNS, SUMMARIZECOLUMNS, CALENDAR or
    /// GENERATESERIES added to the rows being iterated.
    Ext {
        name: String,
    },
    /// EARLIER ( column, n ): the column's value in the row context `n` levels out from
    /// the innermost row of its table.
    Earlier {
        table: Id,
        column: Id,
        n: usize,
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
    /// CONCATENATEX: `expr` per row of `source` as text, joined by `delimiter`, in the
    /// order of `order` (`true` ascending) or else of the text.
    ConcatX {
        source: TableExpr,
        expr: Box<Bound>,
        delimiter: String,
        order: Option<(Box<Bound>, bool)>,
    },
    /// RANKX: 1 + how many rows of `source` have an `expr` beating `value` (`expr` in
    /// the current context when omitted); with `dense`, how many distinct values.
    Rank {
        source: TableExpr,
        expr: Box<Bound>,
        value: Option<Box<Bound>>,
        ascending: bool,
        dense: bool,
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
    "AVERAGEA",
    "CEILING",
    "COUNTAX",
    "CROSSFILTER",
    "DATEVALUE",
    "EXP",
    "FIND",
    "FIRSTNONBLANK",
    "FLOOR",
    "GENERATE",
    "HOUR",
    "IFERROR",
    "ISERROR",
    "LASTNONBLANK",
    "LN",
    "LOG",
    "MAXA",
    "MEDIAN",
    "MEDIANX",
    "MINA",
    "MINUTE",
    "NOW",
    "PERCENTILE.INC",
    "POWER",
    "PRODUCT",
    "PRODUCTX",
    "REPLACE",
    "ROUNDDOWN",
    "ROUNDUP",
    "SEARCH",
    "SIGN",
    "SQRT",
    "STDEV.P",
    "STDEV.S",
    "TREATAS",
    "TRUNC",
    "VALUE",
    "WEEKNUM",
    "YEARFRAC",
];

/// Functions charted evaluates, for docs/dax-support.md.
pub const SUPPORTED: &[&str] = &[
    "ABS",
    "ADDCOLUMNS",
    "ALL",
    "ALLEXCEPT",
    "ALLSELECTED",
    "AND",
    "AVERAGE",
    "AVERAGEX",
    "BLANK",
    "CALCULATE",
    "CALCULATETABLE",
    "CALENDAR",
    "COALESCE",
    "CONCATENATE",
    "CONCATENATEX",
    "COUNT",
    "COUNTA",
    "COUNTBLANK",
    "COUNTROWS",
    "COUNTX",
    "DATE",
    "DATEADD",
    "DATEDIFF",
    "DATESBETWEEN",
    "DATESINPERIOD",
    "DATESMTD",
    "DATESQTD",
    "DATESYTD",
    "DAY",
    "DISTINCT",
    "DISTINCTCOUNT",
    "DIVIDE",
    "EARLIER",
    "EDATE",
    "EOMONTH",
    "FALSE",
    "FILTER",
    "FIRSTDATE",
    "FORMAT",
    "GENERATESERIES",
    "HASONEVALUE",
    "IF",
    "INT",
    "ISBLANK",
    "ISFILTERED",
    "ISINSCOPE",
    "KEEPFILTERS",
    "LASTDATE",
    "LEFT",
    "LEN",
    "LOOKUPVALUE",
    "LOWER",
    "MAX",
    "MAXX",
    "MID",
    "MIN",
    "MINX",
    "MOD",
    "MONTH",
    "NEXTDAY",
    "NEXTMONTH",
    "NEXTQUARTER",
    "NEXTYEAR",
    "NOT",
    "OR",
    "PARALLELPERIOD",
    "PI",
    "PREVIOUSDAY",
    "PREVIOUSMONTH",
    "PREVIOUSQUARTER",
    "PREVIOUSYEAR",
    "QUARTER",
    "RANKX",
    "RELATED",
    "RELATEDTABLE",
    "REMOVEFILTERS",
    "RIGHT",
    "ROUND",
    "SAMEPERIODLASTYEAR",
    "SELECTCOLUMNS",
    "SELECTEDVALUE",
    "SUBSTITUTE",
    "SUM",
    "SUMMARIZE",
    "SUMMARIZECOLUMNS",
    "SUMX",
    "SWITCH",
    "TODAY",
    "TOPN",
    "TOTALMTD",
    "TOTALQTD",
    "TOTALYTD",
    "TRIM",
    "TRUE",
    "UPPER",
    "USERELATIONSHIP",
    "VALUES",
    "WEEKDAY",
    "YEAR",
];

/// Binds `expr`.
pub fn bind(model: &Model, expr: &Expr) -> (Option<Bound>, Vec<Diagnostic>) {
    bind_in(model, expr, None)
}

/// Binds a table expression (a calculated table).
pub fn bind_table(model: &Model, expr: &Expr) -> (Option<TableExpr>, Vec<Diagnostic>) {
    let mut b = Binder {
        model,
        diags: Vec::new(),
        vars: Vec::new(),
        depth: 0,
        exts: Vec::new(),
        carried: Vec::new(),
    };
    let bound = b.table_expr(expr, &[]);
    let ok = b.diags.is_empty();
    (bound.filter(|_| ok), b.diags)
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
        depth: 0,
        exts: Vec::new(),
        carried: Vec::new(),
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
    vars: Vec<Var>,
    /// How many filter or row context changes (CALCULATE, iterators, FILTER) enclose
    /// the expression being bound.
    depth: usize,
    /// Columns added to the rows being iterated (`[Name]`), innermost last.
    exts: Vec<String>,
    /// Other tables' columns the rows being iterated carry (SUMMARIZE's groups).
    carried: Vec<Id>,
}

/// A variable in scope: its definition, and the context depth it was defined at (its
/// value can stand in for the definition only at that same depth).
struct Var {
    name: String,
    value: Expr,
    depth: usize,
    /// A table (`VAR period = DATESINPERIOD ( … )`): bound where it's used as one.
    table: bool,
}

/// Functions that return tables (for telling a table variable from a scalar one).
const TABLE_FUNCTIONS: &[&str] = &[
    "VALUES",
    "DISTINCT",
    "ALL",
    "ALLSELECTED",
    "FILTER",
    "TOPN",
    "CALCULATETABLE",
    "RELATEDTABLE",
    "ADDCOLUMNS",
    "SELECTCOLUMNS",
    "SUMMARIZE",
    "SUMMARIZECOLUMNS",
    "CALENDAR",
    "GENERATESERIES",
    "SAMEPERIODLASTYEAR",
    "DATEADD",
    "PARALLELPERIOD",
    "DATESYTD",
    "DATESQTD",
    "DATESMTD",
    "DATESINPERIOD",
    "DATESBETWEEN",
    "PREVIOUSDAY",
    "PREVIOUSMONTH",
    "PREVIOUSQUARTER",
    "PREVIOUSYEAR",
    "NEXTDAY",
    "NEXTMONTH",
    "NEXTQUARTER",
    "NEXTYEAR",
];

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
    /// `name, expression` pairs of SUMMARIZE and SUMMARIZECOLUMNS: each expression is
    /// evaluated per row of `rows_of` in that row's filter context (CALCULATE ( e )).
    fn pairs(
        &mut self,
        args: &[Expr],
        rows_of: &TableExpr,
        rows: Rows<'a, '_>,
    ) -> Option<Vec<(String, Bound)>> {
        if !args.len().is_multiple_of(2) {
            return self
                .err(
                    args[args.len() - 1].span,
                    "names and expressions go in pairs",
                )
                .and(None);
        }
        let mut cols = Vec::new();
        for pair in args.chunks(2) {
            let ExprKind::Text(name) = &pair[0].kind else {
                return self
                    .err(pair[0].span, "the new column's name, as text")
                    .and(None);
            };
            let x = self.with_row(rows_of, |b| b.expr(&pair[1], rows))?;
            cols.push((
                name.clone(),
                Bound::Calculate {
                    expr: Box::new(x),
                    filters: Vec::new(),
                },
            ));
        }
        Some(cols)
    }

    /// The row contexts inside an iterator over `source`: its model table, if it has one
    /// (CALENDAR and GENERATESERIES rows only have added columns).
    fn inner_rows<'r>(
        &mut self,
        source: &TableExpr,
        rows: Rows<'a, 'r>,
        span: Span,
        name: &str,
    ) -> Option<Vec<&'a Table>> {
        let mut inner = rows.to_vec();
        match source.table().and_then(|t| self.model.table(t)) {
            Some(t) => inner.push(t),
            None if !source.ext_names().is_empty() => {}
            None => {
                return self
                    .err(
                        span,
                        format!("{name} over a list of values isn't supported yet"),
                    )
                    .and(None);
            }
        }
        Some(inner)
    }

    /// Binds inside the row context of `source`: its added and carried columns can be
    /// named.
    fn with_row<T>(&mut self, source: &TableExpr, f: impl FnOnce(&mut Self) -> T) -> T {
        let (ne, nc) = (self.exts.len(), self.carried.len());
        self.exts.extend(source.ext_names());
        self.carried.extend(source.carried());
        let out = self.nested(f);
        self.exts.truncate(ne);
        self.carried.truncate(nc);
        out
    }

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
            ExprKind::Omitted => self.err(e.span, "this argument is missing").and(None),
            ExprKind::Table(name) => {
                if let Some(v) = self
                    .vars
                    .iter()
                    .rev()
                    .find(|v| v.name.eq_ignore_ascii_case(name))
                {
                    if v.table {
                        let n = v.name.clone();
                        return self.err(
                            e.span,
                            format!("{n} is a table; use it inside CALCULATE, an iterator such as SUMX, or COUNTROWS"),
                        );
                    }
                    return Some(Bound::Var(v.name.clone()));
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
                // A table variable is bound where it's used (checked here once).
                if self.is_table(value) {
                    self.table_expr(value, rows)?;
                    self.vars.push(Var {
                        name: name.clone(),
                        value: (**value).clone(),
                        depth: self.depth,
                        table: true,
                    });
                    let b = self.expr(body, rows);
                    self.vars.pop();
                    return b;
                }
                let v = self.expr(value, rows);
                self.vars.push(Var {
                    name: name.clone(),
                    value: (**value).clone(),
                    depth: self.depth,
                    table: false,
                });
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
                if let Some(e) = self
                    .exts
                    .iter()
                    .rev()
                    .find(|e| e.eq_ignore_ascii_case(name))
                {
                    return Some(Bound::Ext { name: e.clone() });
                }
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
        if rows.iter().any(|r| r.id == t.id) || self.carried.contains(&c.id) {
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

    /// A date column argument of a time-intelligence function.
    fn date_column(&mut self, e: &Expr) -> Option<(&'a Table, &'a charted_model::Column)> {
        let (t, c) = self.column_arg(e)?;
        if !matches!(
            c.data_type,
            DataType::Date | DataType::DateTime | DataType::DateTimeTz
        ) {
            return self
                .err(
                    e.span,
                    format!("'{}'[{}] isn't a date column; time intelligence needs the date column of a date table", t.name, c.name),
                )
                .and(None);
        }
        Some((t, c))
    }

    /// A whole number argument, such as DATEADD's -1.
    fn whole(&mut self, e: &Expr) -> Option<i32> {
        let v = match &e.kind {
            ExprKind::Number(n) => Some(*n),
            ExprKind::Unary {
                op: UnaryOp::Neg,
                expr,
            } => match expr.kind {
                ExprKind::Number(n) => Some(-n),
                _ => None,
            },
            _ => None,
        };
        match v {
            Some(n) if n.fract() == 0.0 && n.abs() < 10_000.0 => Some(n as i32),
            _ => self
                .err(e.span, "this needs a whole number, such as -1")
                .and(None),
        }
    }

    /// DAY, MONTH, QUARTER or YEAR.
    fn period(&mut self, e: &Expr) -> Option<Period> {
        let name = match &e.kind {
            ExprKind::Table(n) => Some(n.as_str()),
            ExprKind::Ref { table: None, name } => Some(name.as_str()),
            _ => None,
        };
        match name.map(str::to_ascii_uppercase).as_deref() {
            Some("DAY") => Some(Period::Day),
            Some("MONTH") => Some(Period::Month),
            Some("QUARTER") => Some(Period::Quarter),
            Some("YEAR") => Some(Period::Year),
            _ => self
                .err(e.span, "this needs DAY, MONTH, QUARTER or YEAR")
                .and(None),
        }
    }

    /// A year-end date such as "6/30" (month/day) or "30-6" when the day comes first.
    fn year_end(&mut self, e: &Expr) -> Option<(u32, u32)> {
        let parsed = match &e.kind {
            ExprKind::Text(t) => {
                let parts: Vec<u32> = t
                    .split(['/', '-', ' ', '.'])
                    .filter(|p| !p.is_empty())
                    .map_while(|p| p.parse().ok())
                    .collect();
                match parts[..] {
                    [m, d] if (1..=12).contains(&m) && (1..=31).contains(&d) => Some((m, d)),
                    [d, m] if (1..=12).contains(&m) && (1..=31).contains(&d) => Some((m, d)),
                    _ => None,
                }
            }
            _ => None,
        };
        parsed.or_else(|| {
            self.err(e.span, "the year end is text such as \"6/30\" (month/day)")
                .and(None)
        })
    }

    /// SAMEPERIODLASTYEAR, DATEADD, PARALLELPERIOD, DATESYTD/QTD/MTD, PREVIOUS… and NEXT….
    fn dates(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
        rows: Rows<'a, '_>,
    ) -> Option<TableExpr> {
        let (min, max) = match name {
            "DATEADD" | "PARALLELPERIOD" | "DATESBETWEEN" => (3, 3),
            "DATESINPERIOD" => (4, 4),
            "DATESYTD" => (1, 2),
            _ => (1, 1),
        };
        if !self.arity(name, args, span, min, max) {
            return None;
        }
        let (t, c) = self.date_column(&args[0])?;
        let unit = |n: &str| match &n[n.len() - 3..] {
            "DAY" => Period::Day,
            "NTH" => Period::Month,
            "TER" => Period::Quarter,
            _ => Period::Year,
        };
        let op = match name {
            "SAMEPERIODLASTYEAR" => DateOp::Shift(-1, Period::Year),
            "DATEADD" => DateOp::Shift(self.whole(&args[1])?, self.period(&args[2])?),
            "PARALLELPERIOD" => DateOp::Parallel(self.whole(&args[1])?, self.period(&args[2])?),
            "DATESYTD" => DateOp::ToDate(
                Period::Year,
                match args.get(1) {
                    Some(y) => Some(self.year_end(y)?),
                    None => None,
                },
            ),
            "DATESINPERIOD" => {
                let anchor = self.anchor(&args[1], &c.id, false, rows);
                let n = self.whole(&args[2]);
                let p = self.period(&args[3]);
                DateOp::InPeriod(anchor?, n?, p?)
            }
            "DATESBETWEEN" => {
                let start = self.anchor(&args[1], &c.id, true, rows);
                let end = self.anchor(&args[2], &c.id, true, rows);
                DateOp::Between(start?, end?)
            }
            "DATESQTD" => DateOp::ToDate(Period::Quarter, None),
            "DATESMTD" => DateOp::ToDate(Period::Month, None),
            n if n.starts_with("PREVIOUS") => DateOp::Previous(unit(n)),
            n => DateOp::Next(unit(n)),
        };
        Some(TableExpr::Dates {
            table: t.id.clone(),
            column: c.id.clone(),
            op,
        })
    }

    /// Binds inside a filter or row context change (see `Var`).
    fn nested<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.depth += 1;
        let out = f(self);
        self.depth -= 1;
        out
    }

    /// A date argument of DATESINPERIOD or DATESBETWEEN: MAX, MIN, LASTDATE or
    /// FIRSTDATE of the date column itself, a DATE of constants, BLANK (when `open` is
    /// allowed), a variable defined as one of these in the same context, or any other
    /// date expression.
    fn anchor(&mut self, e: &Expr, column: &Id, open: bool, rows: Rows<'a, '_>) -> Option<Anchor> {
        self.anchor_in(e, column, open, self.vars.len(), rows)
    }

    /// `anchor` with only the first `scope` variables visible (a variable's definition
    /// sees only those defined before it).
    fn anchor_in(
        &mut self,
        e: &Expr,
        column: &Id,
        open: bool,
        scope: usize,
        rows: Rows<'a, '_>,
    ) -> Option<Anchor> {
        let number = |e: &Expr| match &e.kind {
            ExprKind::Number(_) => true,
            ExprKind::Unary {
                op: UnaryOp::Neg,
                expr,
            } => matches!(expr.kind, ExprKind::Number(_)),
            _ => false,
        };
        if let ExprKind::Call { name, args } = &e.kind {
            let upper = name.to_ascii_uppercase();
            match (upper.as_str(), args.len()) {
                ("MAX" | "LASTDATE" | "MIN" | "FIRSTDATE", 1)
                    if matches!(&args[0].kind, ExprKind::Ref { table: Some(_), .. }) =>
                {
                    let (_, c) = self.column_arg(&args[0])?;
                    if &c.id == column {
                        return Some(if matches!(upper.as_str(), "MAX" | "LASTDATE") {
                            Anchor::Last
                        } else {
                            Anchor::First
                        });
                    }
                }
                ("DATE", 3) if args.iter().all(number) => {
                    let y = self.whole(&args[0])?;
                    let m = self.whole(&args[1])?;
                    let d = self.whole(&args[2])?;
                    return Some(Anchor::Date(y, m, d));
                }
                ("BLANK", 0) if open => return Some(Anchor::Open),
                _ => {}
            }
        }
        if let ExprKind::Table(name) | ExprKind::Ref { table: None, name } = &e.kind {
            let found = self.vars[..scope]
                .iter()
                .rposition(|v| v.name.eq_ignore_ascii_case(name));
            // A variable from the same context stands for its definition.
            if let Some(i) = found
                && self.vars[i].depth == self.depth
            {
                let value = self.vars[i].value.clone();
                return self.anchor_in(&value, column, open, i, rows);
            }
        }
        // Anything else is a date expression evaluated in the outer context.
        Some(Anchor::Expr(Box::new(self.expr(e, rows)?)))
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
    /// Whether an expression is a table (a table function, a table, a table variable).
    fn is_table(&self, e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Call { name, .. } => {
                TABLE_FUNCTIONS.contains(&name.to_ascii_uppercase().as_str())
            }
            ExprKind::Table(name) | ExprKind::Ref { table: None, name } => {
                match self
                    .vars
                    .iter()
                    .rev()
                    .find(|v| v.name.eq_ignore_ascii_case(name))
                {
                    Some(v) => v.table,
                    None => {
                        matches!(e.kind, ExprKind::Table(_))
                            && self.model.table_by_name(name).is_some()
                    }
                }
            }
            _ => false,
        }
    }

    /// A table variable used as a table: its definition, bound here (with the variables
    /// it could see), when this is the context it was defined in.
    fn table_var(
        &mut self,
        name: &str,
        span: Span,
        rows: Rows<'a, '_>,
    ) -> Option<Option<TableExpr>> {
        let i = self
            .vars
            .iter()
            .rposition(|v| v.name.eq_ignore_ascii_case(name))?;
        if !self.vars[i].table {
            return None;
        }
        if self.vars[i].depth != self.depth {
            self.err(span, format!("charted can only use the table variable {name} where it's defined, not inside another CALCULATE or iterator"));
            return Some(None);
        }
        let value = self.vars[i].value.clone();
        let later = self.vars.split_off(i);
        let t = self.table_expr(&value, rows);
        self.vars.extend(later);
        Some(t)
    }

    fn table_expr(&mut self, e: &Expr, rows: Rows<'a, '_>) -> Option<TableExpr> {
        if let ExprKind::Table(name) | ExprKind::Ref { table: None, name } = &e.kind
            && let Some(t) = self.table_var(name, e.span, rows)
        {
            return t;
        }
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
                    "ALL" | "REMOVEFILTERS" => {
                        let (table, columns) = self.all_args(&upper, args, e.span)?;
                        Some(TableExpr::All { table, columns })
                    }
                    "ALLSELECTED" => {
                        let (table, columns) = self.all_args(&upper, args, e.span)?;
                        Some(TableExpr::AllSelected { table, columns })
                    }
                    "ADDCOLUMNS" | "SELECTCOLUMNS" => {
                        if !self.arity(&upper, args, e.span, 3, usize::MAX) {
                            return None;
                        }
                        if args.len().is_multiple_of(2) {
                            return self.err(e.span, format!("{upper} takes a table, then names and expressions in pairs")).and(None);
                        }
                        let source = self.table_expr(&args[0], rows)?;
                        let inner = self.inner_rows(&source, rows, args[0].span, &upper)?;
                        let mut cols = Vec::new();
                        for pair in args[1..].chunks(2) {
                            let ExprKind::Text(name) = &pair[0].kind else {
                                return self.err(pair[0].span, "the new column's name, as text").and(None);
                            };
                            let x = self.with_row(&source, |b| b.expr(&pair[1], &inner))?;
                            cols.push((name.clone(), x));
                        }
                        Some(if upper == "ADDCOLUMNS" {
                            TableExpr::AddColumns { source: Box::new(source), cols }
                        } else {
                            TableExpr::SelectColumns { source: Box::new(source), cols }
                        })
                    }
                    "SUMMARIZE" => {
                        if !self.arity(&upper, args, e.span, 2, usize::MAX) {
                            return None;
                        }
                        let source = self.table_expr(&args[0], rows)?;
                        let first = args.iter().skip(1).position(|a| matches!(a.kind, ExprKind::Text(_))).map(|i| i + 1).unwrap_or(args.len());
                        let mut group = Vec::new();
                        for a in &args[1..first] {
                            let (t, c) = self.column_arg(a)?;
                            group.push((t.id.clone(), c.id.clone()));
                        }
                        let table = Box::new(TableExpr::Summarize { source: Box::new(source), group });
                        if first == args.len() {
                            return Some(*table);
                        }
                        // Added columns are evaluated per group, as CALCULATE would.
                        let cols = self.pairs(&args[first..], &table, rows)?;
                        Some(TableExpr::AddColumns { source: table, cols })
                    }
                    "SUMMARIZECOLUMNS" => {
                        let first = args.iter().position(|a| matches!(a.kind, ExprKind::Text(_))).unwrap_or(args.len());
                        let mut group = Vec::new();
                        let mut filters = Vec::new();
                        for a in &args[..first] {
                            if matches!(a.kind, ExprKind::Ref { table: Some(_), .. }) {
                                let (t, c) = self.column_arg(a)?;
                                group.push((t.id.clone(), c.id.clone()));
                            } else {
                                filters.push(self.filter_arg(a, rows)?);
                            }
                        }
                        if group.is_empty() {
                            return self.err(e.span, "SUMMARIZECOLUMNS needs at least one column to group by").and(None);
                        }
                        let shape = TableExpr::SummarizeColumns { group: group.clone(), filters: filters.clone(), cols: Vec::new() };
                        let cols = self.pairs(&args[first..], &shape, rows)?;
                        Some(TableExpr::SummarizeColumns { group, filters, cols })
                    }
                    "CALENDAR" => {
                        if !self.arity(&upper, args, e.span, 2, 2) {
                            return None;
                        }
                        let start = self.expr(&args[0], rows);
                        let end = self.expr(&args[1], rows);
                        Some(TableExpr::Calendar { start: Box::new(start?), end: Box::new(end?) })
                    }
                    "GENERATESERIES" => {
                        if !self.arity(&upper, args, e.span, 2, 3) {
                            return None;
                        }
                        let start = self.expr(&args[0], rows);
                        let end = self.expr(&args[1], rows);
                        let step = match args.get(2) {
                            Some(a) => self.expr(a, rows),
                            None => Some(Bound::Number(1.0)),
                        };
                        Some(TableExpr::GenerateSeries { start: Box::new(start?), end: Box::new(end?), step: Box::new(step?) })
                    }
                    "CALCULATETABLE" | "RELATEDTABLE" => {
                        let max = if upper == "RELATEDTABLE" { 1 } else { usize::MAX };
                        if !self.arity(&upper, args, e.span, 1, max) {
                            return None;
                        }
                        let source = self.table_expr(&args[0], rows)?;
                        let filters: Vec<Option<FilterArg>> = args[1..].iter().map(|a| self.filter_arg(a, rows)).collect();
                        Some(TableExpr::CalculateTable {
                            source: Box::new(source),
                            filters: filters.into_iter().collect::<Option<Vec<_>>>()?,
                        })
                    }
                    "TOPN" => {
                        if !self.arity(&upper, args, e.span, 3, usize::MAX) {
                            return None;
                        }
                        let n = self.expr(&args[0], rows);
                        let source = self.table_expr(&args[1], rows)?;
                        let inner = self.inner_rows(&source, rows, args[1].span, "TOPN")?;
                        // Order expressions, each optionally followed by ASC or DESC.
                        let mut order: Vec<(Bound, bool)> = Vec::new();
                        let mut i = 2;
                        while i < args.len() {
                            let o = self.with_row(&source, |b| b.expr(&args[i], &inner))?;
                            let asc = match args.get(i + 1).and_then(order_keyword) {
                                Some(a) => {
                                    i += 1;
                                    a
                                }
                                None => false,
                            };
                            order.push((o, asc));
                            i += 1;
                        }
                        Some(TableExpr::TopN {
                            n: Box::new(n?),
                            source: Box::new(source),
                            order,
                        })
                    }
                    "FILTER" => {
                        if !self.arity(&upper, args, e.span, 2, 2) {
                            return None;
                        }
                        let source = self.table_expr(&args[0], rows)?;
                        let inner = self.inner_rows(&source, rows, args[0].span, "FILTER")?;
                        let condition = self.with_row(&source, |b| b.expr(&args[1], &inner))?;
                        Some(TableExpr::Filter {
                            source: Box::new(source),
                            condition: Box::new(condition),
                        })
                    }
                    "SAMEPERIODLASTYEAR" | "DATEADD" | "PARALLELPERIOD" | "DATESYTD" | "DATESQTD" | "DATESMTD" | "DATESINPERIOD" | "DATESBETWEEN" | "PREVIOUSDAY" | "PREVIOUSMONTH" | "PREVIOUSQUARTER" | "PREVIOUSYEAR" | "NEXTDAY" | "NEXTMONTH" | "NEXTQUARTER" | "NEXTYEAR" => self.dates(&upper, args, e.span, rows),
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
                        "{name} without arguments only works as a CALCULATE filter; name a table or columns"
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
        if let ExprKind::Table(name) | ExprKind::Ref { table: None, name } = &e.kind
            && let Some(t) = self.table_var(name, e.span, rows)
        {
            return Some(FilterArg::Table(t?));
        }
        if let ExprKind::Call { name, args } = &e.kind {
            let upper = name.to_ascii_uppercase();
            match upper.as_str() {
                // Without arguments, they clear the whole model's filters (only allowed
                // as a CALCULATE filter, as in DAX).
                "ALL" | "REMOVEFILTERS" if args.is_empty() => return Some(FilterArg::RemoveAll),
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
                "FILTER" | "VALUES" | "DISTINCT" | "TOPN" | "CALCULATETABLE" | "RELATEDTABLE"
                | "SAMEPERIODLASTYEAR" | "DATEADD" | "PARALLELPERIOD" | "DATESYTD" | "DATESQTD"
                | "DATESMTD" | "DATESINPERIOD" | "DATESBETWEEN" | "PREVIOUSDAY"
                | "PREVIOUSMONTH" | "PREVIOUSQUARTER" | "PREVIOUSYEAR" | "NEXTDAY"
                | "NEXTMONTH" | "NEXTQUARTER" | "NEXTYEAR" => {
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
        let condition = self.nested(|b| b.expr(e, &inner))?;
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
            let inner = self.inner_rows(&source, rows, args[0].span, &upper)?;
            let arg = self.with_row(&source, |b| b.expr(&args[1], &inner))?;
            return Some(Bound::Aggregate {
                func,
                source,
                arg: Some(Box::new(arg)),
            });
        }
        match upper.as_str() {
            "CONCATENATEX" => {
                if !self.arity(&upper, args, span, 2, 5) {
                    return None;
                }
                let source = self.table_expr(&args[0], rows)?;
                let inner = self.inner_rows(&source, rows, args[0].span, "CONCATENATEX")?;
                let expr = self.with_row(&source, |b| b.expr(&args[1], &inner));
                let delimiter = match args.get(2).map(|a| &a.kind) {
                    Some(ExprKind::Text(d)) => d.clone(),
                    Some(ExprKind::Omitted) | None => String::new(),
                    Some(_) => return self.err(args[2].span, "charted needs the delimiter written as text"),
                };
                let order = match args.get(3) {
                    Some(Expr { kind: ExprKind::Omitted, .. }) | None => None,
                    Some(o) => {
                        let o = self.with_row(&source, |b| b.expr(o, &inner))?;
                        let asc = args.get(4).and_then(order_keyword).unwrap_or(true);
                        Some((Box::new(o), asc))
                    }
                };
                Some(Bound::ConcatX {
                    source,
                    expr: Box::new(expr?),
                    delimiter,
                    order,
                })
            }
            "RANKX" => {
                if !self.arity(&upper, args, span, 2, 5) {
                    return None;
                }
                let source = self.table_expr(&args[0], rows)?;
                let inner = self.inner_rows(&source, rows, args[0].span, "RANKX")?;
                let expr = self.with_row(&source, |b| b.expr(&args[1], &inner));
                let value = match args.get(2) {
                    Some(Expr { kind: ExprKind::Omitted, .. }) | None => None,
                    Some(v) => Some(Box::new(self.expr(v, rows)?)),
                };
                let keyword = |e: Option<&Expr>| match e.map(|e| &e.kind) {
                    Some(ExprKind::Table(n) | ExprKind::Ref { table: None, name: n }) => Some(n.to_ascii_uppercase()),
                    Some(ExprKind::Number(n)) => Some(if *n == 0.0 { "DESC".into() } else { "ASC".into() }),
                    Some(ExprKind::Bool(b)) => Some(if *b { "ASC".into() } else { "DESC".into() }),
                    Some(ExprKind::Omitted) | None => Some(String::new()),
                    _ => None,
                };
                let ascending = match keyword(args.get(3)).as_deref() {
                    Some("ASC") => true,
                    Some("DESC" | "") => false,
                    _ => return self.err(args[3].span, "the order is ASC or DESC"),
                };
                let dense = match keyword(args.get(4)).as_deref() {
                    Some("DENSE") => true,
                    Some("SKIP" | "") => false,
                    _ => return self.err(args[4].span, "ties are SKIP or DENSE"),
                };
                Some(Bound::Rank {
                    source,
                    expr: Box::new(expr?),
                    value,
                    ascending,
                    dense,
                })
            }
            "COUNTROWS" => {
                if !self.arity(&upper, args, span, 1, 1) {
                    return None;
                }
                let source = self.table_expr(&args[0], rows)?;
                if source.table().is_none() && source.ext_names().is_empty() {
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
                let expr = self.nested(|b| b.expr(&args[0], rows));
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
            "EARLIER" => {
                if !self.arity(&upper, args, span, 1, 2) {
                    return None;
                }
                let (t, c) = self.column_arg(&args[0])?;
                let n = match args.get(1) {
                    Some(a) => self.whole(a)?,
                    None => 1,
                };
                let levels = rows.iter().filter(|r| r.id == t.id).count();
                if n < 1 || levels <= n as usize {
                    return self.err(
                        span,
                        format!("EARLIER needs {} row contexts on '{}' (an iterator such as FILTER inside a calculated column)", n + 1, t.name),
                    );
                }
                Some(Bound::Earlier {
                    table: t.id.clone(),
                    column: c.id.clone(),
                    n: n as usize,
                })
            }
            "LOOKUPVALUE" => {
                if args.len() < 3 {
                    return self.err(span, "LOOKUPVALUE takes a result column, then search columns and values");
                }
                let (rt, rc) = self.column_arg(&args[0])?;
                let pairs = (args.len() - 1) / 2;
                let alternate = if args.len().is_multiple_of(2) { Some(Box::new(self.expr(&args[args.len() - 1], rows)?)) } else { None };
                // Each value is a variable, evaluated in the calling context; the search
                // columns are FILTER ( ALL ( column ), column = value ) arguments.
                let mut lets = Vec::new();
                let mut filters = Vec::new();
                for i in 0..pairs {
                    let (st, sc) = self.column_arg(&args[1 + 2 * i])?;
                    let value = self.expr(&args[2 + 2 * i], rows)?;
                    let name = format!("__lookup{}_{i}", self.vars.len());
                    lets.push((name.clone(), value));
                    filters.push(FilterArg::Table(TableExpr::Filter {
                        source: Box::new(TableExpr::All {
                            table: st.id.clone(),
                            columns: vec![sc.id.clone()],
                        }),
                        condition: Box::new(Bound::Binary {
                            op: BinaryOp::Eq,
                            left: Box::new(Bound::Column {
                                table: st.id.clone(),
                                column: sc.id.clone(),
                            }),
                            right: Box::new(Bound::Var(name)),
                        }),
                    }));
                }
                let mut out = Bound::Calculate {
                    expr: Box::new(Bound::Info {
                        func: InfoFunc::Lookup,
                        table: rt.id.clone(),
                        column: Some(rc.id.clone()),
                        alternate,
                    }),
                    filters,
                };
                for (name, value) in lets.into_iter().rev() {
                    out = Bound::Let {
                        name,
                        value: Box::new(value),
                        body: Box::new(out),
                    };
                }
                Some(out)
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
            "PI" => self
                .arity(&upper, args, span, 0, 0)
                .then_some(Bound::Number(std::f64::consts::PI)),
            // CONCATENATE is the & operator with two arguments.
            "CONCATENATE" => {
                if !self.arity(&upper, args, span, 2, 2) {
                    return None;
                }
                let l = self.expr(&args[0], rows);
                let r = self.expr(&args[1], rows);
                Some(Bound::Binary {
                    op: BinaryOp::Concat,
                    left: Box::new(l?),
                    right: Box::new(r?),
                })
            }
            "FORMAT" => {
                if !self.arity(&upper, args, span, 2, 2) {
                    return None;
                }
                let value = self.expr(&args[0], rows);
                let ExprKind::Text(f) = &args[1].kind else {
                    return self.err(
                        args[1].span,
                        "charted needs FORMAT's format string written out as text, such as \"0.0%\"",
                    );
                };
                let spec = match crate::format::parse(f) {
                    Ok(spec) => spec,
                    Err(message) => return self.err(args[1].span, message),
                };
                Some(Bound::Call {
                    func: ScalarFunc::Format(spec),
                    args: vec![value?],
                })
            }
            "DATEDIFF" => {
                if !self.arity(&upper, args, span, 3, 3) {
                    return None;
                }
                let a = self.expr(&args[0], rows);
                let b = self.expr(&args[1], rows);
                let p = self.period(&args[2]);
                Some(Bound::Call {
                    func: ScalarFunc::DateDiff(p?),
                    args: vec![a?, b?],
                })
            }
            "TRUE" | "FALSE" => self.arity(&upper, args, span, 0, 0).then_some(Bound::Bool(upper == "TRUE")),
            "TOTALYTD" | "TOTALQTD" | "TOTALMTD" => {
                let ytd = upper == "TOTALYTD";
                if !self.arity(&upper, args, span, 2, if ytd { 4 } else { 3 }) {
                    return None;
                }
                let expr = self.nested(|b| b.expr(&args[0], rows));
                let period = match upper.as_str() {
                    "TOTALYTD" => "DATESYTD",
                    "TOTALQTD" => "DATESQTD",
                    _ => "DATESMTD",
                };
                // TOTALYTD ( e, dates, [filter], [year end] ): the year end is the last text.
                let (filter, year_end) = match &args[2..] {
                    [] => (None, None),
                    [x] if ytd && matches!(x.kind, ExprKind::Text(_)) => (None, Some(x)),
                    [f] => (Some(f), None),
                    [f, y, ..] => (Some(f), Some(y)),
                };
                let mut dargs = vec![args[1].clone()];
                dargs.extend(year_end.cloned());
                let dates = self.dates(period, &dargs, span, rows);
                let mut filters = vec![FilterArg::Table(dates?)];
                if let Some(f) = filter {
                    filters.push(self.filter_arg(f, rows)?);
                }
                Some(Bound::Calculate {
                    expr: Box::new(expr?),
                    filters,
                })
            }
            "FIRSTDATE" | "LASTDATE" => {
                if !self.arity(&upper, args, span, 1, 1) {
                    return None;
                }
                let (t, c) = self.date_column(&args[0])?;
                Some(Bound::Aggregate {
                    func: if upper == "FIRSTDATE" { AggFunc::Min } else { AggFunc::Max },
                    source: TableExpr::Table(t.id.clone()),
                    arg: Some(Box::new(Bound::Column {
                        table: t.id.clone(),
                        column: c.id.clone(),
                    })),
                })
            }
            "VALUES" | "DISTINCT" | "ALL" | "FILTER" | "TOPN" | "CALCULATETABLE" | "RELATEDTABLE" | "ADDCOLUMNS" | "SELECTCOLUMNS" | "SUMMARIZE" | "SUMMARIZECOLUMNS" | "CALENDAR" | "GENERATESERIES" | "REMOVEFILTERS" | "ALLEXCEPT" | "ALLSELECTED" | "KEEPFILTERS"
            | "USERELATIONSHIP" | "SAMEPERIODLASTYEAR" | "DATEADD" | "PARALLELPERIOD" | "DATESYTD" | "DATESQTD" | "DATESMTD" | "DATESINPERIOD" | "DATESBETWEEN" | "PREVIOUSDAY" | "PREVIOUSMONTH" | "PREVIOUSQUARTER" | "PREVIOUSYEAR" | "NEXTDAY" | "NEXTMONTH" | "NEXTQUARTER" | "NEXTYEAR" => self.err(
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
                    "YEAR" => (ScalarFunc::Year, 1, 1),
                    "MONTH" => (ScalarFunc::Month, 1, 1),
                    "DAY" => (ScalarFunc::Day, 1, 1),
                    "QUARTER" => (ScalarFunc::Quarter, 1, 1),
                    "MIN" => (ScalarFunc::Min, 2, 2),
                    "MAX" => (ScalarFunc::Max, 2, 2),
                    "INT" => (ScalarFunc::Int, 1, 1),
                    "UPPER" => (ScalarFunc::Upper, 1, 1),
                    "LOWER" => (ScalarFunc::Lower, 1, 1),
                    "LEN" => (ScalarFunc::Len, 1, 1),
                    "TRIM" => (ScalarFunc::Trim, 1, 1),
                    "LEFT" => (ScalarFunc::Left, 1, 2),
                    "RIGHT" => (ScalarFunc::Right, 1, 2),
                    "MID" => (ScalarFunc::Mid, 3, 3),
                    "SUBSTITUTE" => (ScalarFunc::Substitute, 3, 4),
                    "DATE" => (ScalarFunc::Date, 3, 3),
                    "EOMONTH" => (ScalarFunc::EoMonth, 2, 2),
                    "TODAY" => (ScalarFunc::Today, 0, 0),
                    "MOD" => (ScalarFunc::Mod, 2, 2),
                    "WEEKDAY" => (ScalarFunc::Weekday, 1, 2),
                    "EDATE" => (ScalarFunc::EDate, 2, 2),
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
        Bound::ConcatX {
            source,
            expr,
            order,
            ..
        } => {
            walk_table(source, f);
            walk(expr, f);
            if let Some((o, _)) = order {
                walk(o, f);
            }
        }
        Bound::Rank {
            source,
            expr,
            value,
            ..
        } => {
            walk_table(source, f);
            walk(expr, f);
            if let Some(v) = value {
                walk(v, f);
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

/// ASC/DESC (or 1/0, TRUE/FALSE) as an order argument: `true` ascending.
fn order_keyword(e: &Expr) -> Option<bool> {
    match &e.kind {
        ExprKind::Table(n)
        | ExprKind::Ref {
            table: None,
            name: n,
        } => match n.to_ascii_uppercase().as_str() {
            "ASC" => Some(true),
            "DESC" => Some(false),
            _ => None,
        },
        ExprKind::Number(n) => Some(*n != 0.0),
        ExprKind::Bool(b) => Some(*b),
        _ => None,
    }
}

fn walk_table(t: &TableExpr, f: &mut dyn FnMut(&Bound)) {
    match t {
        TableExpr::Filter { source, condition } => {
            walk_table(source, f);
            walk(condition, f);
        }
        TableExpr::List(items) => items.iter().for_each(|a| walk(a, f)),
        TableExpr::CalculateTable { source, filters } => {
            walk_table(source, f);
            filters.iter().for_each(|fa| walk_filter(fa, f));
        }
        TableExpr::AddColumns { source, cols } | TableExpr::SelectColumns { source, cols } => {
            walk_table(source, f);
            cols.iter().for_each(|(_, x)| walk(x, f));
        }
        TableExpr::Summarize { source, .. } => walk_table(source, f),
        TableExpr::SummarizeColumns { filters, cols, .. } => {
            filters.iter().for_each(|fa| walk_filter(fa, f));
            cols.iter().for_each(|(_, x)| walk(x, f));
        }
        TableExpr::Calendar { start, end } => {
            walk(start, f);
            walk(end, f);
        }
        TableExpr::GenerateSeries { start, end, step } => {
            walk(start, f);
            walk(end, f);
            walk(step, f);
        }
        TableExpr::TopN { n, source, order } => {
            walk(n, f);
            walk_table(source, f);
            order.iter().for_each(|(o, _)| walk(o, f));
        }
        TableExpr::Dates { op, .. } => {
            let anchors: Vec<&Anchor> = match op {
                DateOp::InPeriod(a, ..) => vec![a],
                DateOp::Between(a, b) => vec![a, b],
                _ => Vec::new(),
            };
            for a in anchors {
                if let Anchor::Expr(e) = a {
                    walk(e, f);
                }
            }
        }
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
