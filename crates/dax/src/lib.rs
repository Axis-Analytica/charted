//! DAX for charted: a lexer, an error-tolerant parser and a binder that resolves names
//! against the model. Free of I/O so it also builds for WebAssembly (the editor's
//! language service).

pub mod bind;
pub mod calcitem;
pub mod catalog;
pub mod format;
pub mod ide;
pub mod lexer;
pub mod parser;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use bind::{
    AggFunc, Anchor, Bound, DateOp, FilterArg, InfoFunc, Period, ScalarFunc, TableExpr, bind,
    bind_in,
};
pub use format::{DatePart, FormatSpec};
pub use parser::{BinaryOp, Expr, ExprKind, Parsed, UnaryOp, parse};

/// A byte range in the formula text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn to(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Diagnostic {
    pub message: String,
    pub span: Span,
    /// A second location the message refers to, such as an unclosed parenthesis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related: Option<Span>,
}

impl Diagnostic {
    pub fn error(span: Span, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            span,
            related: None,
        }
    }
}

/// Parses and binds a formula, returning every problem found.
pub fn check(model: &charted_model::Model, src: &str) -> (Option<Bound>, Vec<Diagnostic>) {
    check_in(model, src, None)
}

/// Parses and binds a table expression (a calculated table).
pub fn check_table(
    model: &charted_model::Model,
    src: &str,
) -> (Option<TableExpr>, Vec<Diagnostic>) {
    let parsed = parse(src);
    if !parsed.diagnostics.is_empty() {
        return (None, parsed.diagnostics);
    }
    bind::bind_table(model, &parsed.expr)
}

/// Parses and binds a calculation item's expression.
pub fn check_item(model: &charted_model::Model, src: &str) -> Vec<Diagnostic> {
    let parsed = parse(src);
    if !parsed.diagnostics.is_empty() {
        return parsed.diagnostics;
    }
    bind::bind_item(model, &parsed.expr).1
}

/// Parses and binds a formula evaluated per row of `row_table` (a calculated column).
pub fn check_in(
    model: &charted_model::Model,
    src: &str,
    row_table: Option<&str>,
) -> (Option<Bound>, Vec<Diagnostic>) {
    let parsed = parse(src);
    if !parsed.diagnostics.is_empty() {
        return (None, parsed.diagnostics);
    }
    bind_in(model, &parsed.expr, row_table)
}
