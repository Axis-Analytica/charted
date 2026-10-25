//! Resolves names in a parsed DAX expression against the model and checks the parts
//! charted can evaluate. The result refers to tables, columns and measures by id.
//!
//! Supported so far: literals, arithmetic, comparison and logic operators, `&`, the
//! aggregates SUM AVERAGE MIN MAX COUNT COUNTA COUNTBLANK COUNTROWS DISTINCTCOUNT, the
//! iterators SUMX AVERAGEX MINX MAXX COUNTX over a table's own columns, DIVIDE, IF,
//! BLANK, ABS, ROUND, COALESCE, AND, OR, NOT, and references to other measures.
//! Anything else gets a diagnostic saying it isn't supported yet.

use charted_model::{DataType, Id, Model, Table};

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
}

#[derive(Debug, Clone, PartialEq)]
pub enum Bound {
    Number(f64),
    Text(String),
    Bool(bool),
    Blank,
    /// A column of the current row (only inside an aggregate or iterator).
    Column {
        table: Id,
        column: Id,
    },
    Measure {
        table: Id,
        measure: Id,
    },
    /// An aggregate over the rows of `table` visible in the filter context. `arg` is
    /// evaluated per row (a column, or an iterator's expression); COUNTROWS has none.
    Aggregate {
        func: AggFunc,
        table: Id,
        arg: Option<Box<Bound>>,
    },
    Call {
        func: ScalarFunc,
        args: Vec<Bound>,
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
const KNOWN: &[&str] = &[
    "ADDCOLUMNS",
    "ALL",
    "ALLEXCEPT",
    "ALLSELECTED",
    "ALLNOBLANKROW",
    "AVERAGEA",
    "CALCULATE",
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
    "DISTINCT",
    "EARLIER",
    "EDATE",
    "EOMONTH",
    "EXP",
    "FILTER",
    "FIND",
    "FIRSTDATE",
    "FIRSTNONBLANK",
    "FLOOR",
    "FORMAT",
    "GENERATE",
    "HASONEVALUE",
    "HOUR",
    "IFERROR",
    "INT",
    "ISBLANK",
    "ISERROR",
    "ISFILTERED",
    "ISINSCOPE",
    "KEEPFILTERS",
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
    "RELATED",
    "RELATEDTABLE",
    "REMOVEFILTERS",
    "REPLACE",
    "RIGHT",
    "ROUNDDOWN",
    "ROUNDUP",
    "SAMEPERIODLASTYEAR",
    "SEARCH",
    "SELECTEDVALUE",
    "SIGN",
    "SQRT",
    "STDEV.P",
    "STDEV.S",
    "SUBSTITUTE",
    "SUMMARIZE",
    "SUMMARIZECOLUMNS",
    "SWITCH",
    "TODAY",
    "TOPN",
    "TOTALMTD",
    "TOTALQTD",
    "TOTALYTD",
    "TREATAS",
    "TRIM",
    "TRUNC",
    "UPPER",
    "USERELATIONSHIP",
    "VALUE",
    "VALUES",
    "WEEKDAY",
    "WEEKNUM",
    "YEAR",
    "YEARFRAC",
];

/// Binds `expr`. `home` is the table a measure lives in (used only in messages).
pub fn bind(model: &Model, expr: &Expr) -> (Option<Bound>, Vec<Diagnostic>) {
    let mut b = Binder {
        model,
        diags: Vec::new(),
    };
    let bound = b.expr(expr, None);
    let ok = b.diags.is_empty();
    (bound.filter(|_| ok), b.diags)
}

struct Binder<'a> {
    model: &'a Model,
    diags: Vec<Diagnostic>,
}

/// The table rows are iterated over, inside an aggregate or iterator.
type RowContext<'a> = Option<&'a Table>;

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

    fn expr(&mut self, e: &Expr, row: RowContext<'a>) -> Option<Bound> {
        match &e.kind {
            ExprKind::Number(n) => Some(Bound::Number(*n)),
            ExprKind::Text(s) => Some(Bound::Text(s.clone())),
            ExprKind::Bool(v) => Some(Bound::Bool(*v)),
            ExprKind::Error => None,
            ExprKind::Table(name) => {
                self.table(name, e.span)?;
                self.err(
                    e.span,
                    format!("'{name}' is a table; use it inside a function such as COUNTROWS"),
                )
            }
            ExprKind::Ref { table, name } => self.reference(table.as_deref(), name, e.span, row),
            ExprKind::Unary { op, expr } => {
                let inner = self.expr(expr, row)?;
                Some(Bound::Unary {
                    op: *op,
                    expr: Box::new(inner),
                })
            }
            ExprKind::Binary { op, left, right } => {
                let l = self.expr(left, row);
                let r = self.expr(right, row);
                Some(Bound::Binary {
                    op: *op,
                    left: Box::new(l?),
                    right: Box::new(r?),
                })
            }
            ExprKind::Call { name, args } => self.call(name, args, e.span, row),
        }
    }

    fn reference(
        &mut self,
        table: Option<&str>,
        name: &str,
        span: Span,
        row: RowContext<'a>,
    ) -> Option<Bound> {
        let column_in = |t: &'a Table| t.column_by_name(name).map(|c| (t, c));
        let (t, c) = match table {
            Some(tn) => {
                let t = self.table(tn, span)?;
                if let Some(m) = t
                    .measures
                    .iter()
                    .find(|m| m.name.eq_ignore_ascii_case(name))
                {
                    return self.measure_ref(t, m, span, row);
                }
                match column_in(t) {
                    Some(tc) => tc,
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
                    return self.measure_ref(t, m, span, row);
                }
                match row.and_then(column_in) {
                    Some(tc) => tc,
                    None => {
                        return self.err(span, format!("there's no measure named [{name}]"));
                    }
                }
            }
        };
        match row {
            None => self.err(
                span,
                format!(
                    "a single value for column '{}'[{}] can't be determined here; wrap it in an aggregation such as SUM",
                    t.name, c.name
                ),
            ),
            Some(r) if r.id != t.id => self.err(
                span,
                format!(
                    "only columns of '{}' can be used here (RELATED isn't supported yet)",
                    r.name
                ),
            ),
            Some(_) => Some(Bound::Column {
                table: t.id.clone(),
                column: c.id.clone(),
            }),
        }
    }

    fn measure_ref(
        &mut self,
        t: &Table,
        m: &charted_model::Measure,
        span: Span,
        row: RowContext<'a>,
    ) -> Option<Bound> {
        if row.is_some() {
            return self.err(
                span,
                format!(
                    "measures can't be used inside iterators yet ([{}] needs context transition)",
                    m.name
                ),
            );
        }
        Some(Bound::Measure {
            table: t.id.clone(),
            measure: m.id.clone(),
        })
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

    fn call(
        &mut self,
        name: &str,
        args: &[Expr],
        span: Span,
        row: RowContext<'a>,
    ) -> Option<Bound> {
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
            if row.is_some() {
                return self.err(
                    span,
                    format!("{upper} inside an iterator isn't supported yet"),
                );
            }
            let ExprKind::Ref { .. } = &args[0].kind else {
                return self.err(
                    args[0].span,
                    format!("{upper} needs a column, such as Sales[Amount]"),
                );
            };
            let (t, col) = self.column_arg(&args[0])?;
            let numeric = col.data_type.is_numeric();
            if matches!(func, AggFunc::Sum | AggFunc::Average) && !numeric {
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
                table: t.id.clone(),
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
            if row.is_some() {
                return self.err(span, "nested iterators aren't supported yet");
            }
            let ExprKind::Table(tn) = &args[0].kind else {
                return self.err(
                    args[0].span,
                    format!("{upper} iterates a table; give a table name such as Sales (FILTER isn't supported yet)"),
                );
            };
            let t = self.table(tn, args[0].span)?;
            let inner = self.expr(&args[1], Some(t))?;
            return Some(Bound::Aggregate {
                func,
                table: t.id.clone(),
                arg: Some(Box::new(inner)),
            });
        }
        match upper.as_str() {
            "COUNTROWS" => {
                if !self.arity(&upper, args, span, 1, 1) {
                    return None;
                }
                let ExprKind::Table(tn) = &args[0].kind else {
                    return self.err(args[0].span, "COUNTROWS needs a table name, such as Sales");
                };
                let t = self.table(tn, args[0].span)?;
                Some(Bound::Aggregate {
                    func: AggFunc::CountRows,
                    table: t.id.clone(),
                    arg: None,
                })
            }
            "BLANK" => self.arity(&upper, args, span, 0, 0).then_some(Bound::Blank),
            "TRUE" | "FALSE" => self
                .arity(&upper, args, span, 0, 0)
                .then_some(Bound::Bool(upper == "TRUE")),
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
                    // MIN/MAX with two scalars are not aggregates.
                    "MIN" | "MAX" => {
                        return self
                            .err(span, format!("{upper} of two values isn't supported yet"));
                    }
                    _ if KNOWN.contains(&upper.as_str()) => {
                        return self.err(span, format!("{upper} isn't supported by charted yet"));
                    }
                    _ => return self.err(span, format!("{name} isn't a DAX function")),
                };
                if !self.arity(&upper, args, span, min, max) {
                    return None;
                }
                let bound: Vec<Option<Bound>> = args.iter().map(|a| self.expr(a, row)).collect();
                let args = bound.into_iter().collect::<Option<Vec<_>>>()?;
                Some(Bound::Call { func, args })
            }
        }
    }

    /// The column named by an aggregate's argument (`Sales[Amount]`).
    fn column_arg(&mut self, e: &Expr) -> Option<(&'a Table, &'a charted_model::Column)> {
        let ExprKind::Ref { table, name } = &e.kind else {
            return None;
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
    match bound {
        Bound::Measure { measure, .. } => out.push(measure.clone()),
        Bound::Aggregate { arg: Some(a), .. } => measure_refs(a, out),
        Bound::Call { args, .. } => args.iter().for_each(|a| measure_refs(a, out)),
        Bound::Unary { expr, .. } => measure_refs(expr, out),
        Bound::Binary { left, right, .. } => {
            measure_refs(left, out);
            measure_refs(right, out);
        }
        _ => {}
    }
}
