//! A Pratt parser for DAX expressions. It always returns a tree: where the text is
//! broken it inserts `Error` nodes, records a diagnostic and carries on, so editors get
//! every problem at once.

use crate::lexer::{Tok, Token, lex};
use crate::{Diagnostic, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Plus,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Concat,
    Eq,
    /// `==`: equal, and blank only equals blank.
    StrictEq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    /// `x IN { … }`
    In,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Number(f64),
    Text(String),
    Bool(bool),
    /// `Table[Column]`, or `[Name]` (a measure, or a column of the row context).
    Ref {
        table: Option<String>,
        name: String,
    },
    /// A table name on its own, as in `COUNTROWS ( Sales )`.
    Table(String),
    Call {
        name: String,
        args: Vec<Expr>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// `VAR name = value` … `RETURN body` (one variable per node, nested).
    Var {
        name: String,
        name_span: Span,
        value: Box<Expr>,
        body: Box<Expr>,
    },
    /// A table constructor `{ a, b, c }` (one column).
    List(Vec<Expr>),
    Error,
    /// An argument left out (`f ( a, , c )`).
    Omitted,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

pub struct Parsed {
    pub expr: Expr,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn parse(src: &str) -> Parsed {
    let mut p = Parser {
        toks: lex(src),
        pos: 0,
        diags: Vec::new(),
    };
    let expr = if matches!(p.peek(), Tok::Eof) {
        p.error(p.span(), "the formula is empty");
        Expr {
            kind: ExprKind::Error,
            span: p.span(),
        }
    } else {
        p.expr(0)
    };
    if !matches!(p.peek(), Tok::Eof) {
        let span = p.span();
        p.error(
            span,
            &format!(
                "unexpected {} after the end of the formula",
                describe(p.peek())
            ),
        );
    }
    Parsed {
        expr,
        diagnostics: p.diags,
    }
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Number(n) => format!("number {n}"),
        Tok::Text(_) => "text".into(),
        Tok::Ident(s) => format!("\"{s}\""),
        Tok::Quoted(s) => format!("'{s}'"),
        Tok::Bracket(s) => format!("[{s}]"),
        Tok::Unknown(s) if s.starts_with(['"', '\'', '[']) => {
            format!("unclosed {}", &s[..1])
        }
        Tok::Unknown(s) => format!("\"{s}\""),
        Tok::Eof => "end of formula".into(),
        Tok::LParen => "\"(\"".into(),
        Tok::RParen => "\")\"".into(),
        Tok::Comma => "\",\"".into(),
        _ => "operator".into(),
    }
}

struct Parser {
    toks: Vec<Token>,
    pos: usize,
    diags: Vec<Diagnostic>,
}

/// Binding power of infix operators (higher binds tighter), as in DAX.
fn infix(t: &Tok) -> Option<(u8, BinaryOp)> {
    Some(match t {
        Tok::OrOr => (1, BinaryOp::Or),
        Tok::AndAnd => (2, BinaryOp::And),
        Tok::Eq => (4, BinaryOp::Eq),
        Tok::EqEq => (4, BinaryOp::StrictEq),
        Tok::Ne => (4, BinaryOp::Ne),
        Tok::Lt => (4, BinaryOp::Lt),
        Tok::Le => (4, BinaryOp::Le),
        Tok::Gt => (4, BinaryOp::Gt),
        Tok::Ge => (4, BinaryOp::Ge),
        Tok::Ident(s) if s.eq_ignore_ascii_case("IN") => (4, BinaryOp::In),
        Tok::Amp => (5, BinaryOp::Concat),
        Tok::Plus => (6, BinaryOp::Add),
        Tok::Minus => (6, BinaryOp::Sub),
        Tok::Star => (7, BinaryOp::Mul),
        Tok::Slash => (7, BinaryOp::Div),
        Tok::Caret => (9, BinaryOp::Pow),
        _ => return None,
    })
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn span(&self) -> Span {
        self.toks[self.pos].span
    }

    fn bump(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    fn error(&mut self, span: Span, message: &str) {
        // One diagnostic per position is enough.
        if self.diags.last().is_some_and(|d| d.span == span) {
            return;
        }
        self.diags.push(Diagnostic::error(span, message));
    }

    fn expr(&mut self, min: u8) -> Expr {
        let mut left = self.prefix();
        while let Some((bp, op)) = infix(self.peek()) {
            if bp < min {
                break;
            }
            self.bump();
            // `^` is right-associative; the rest associate to the left.
            let right = self.expr(if op == BinaryOp::Pow { bp } else { bp + 1 });
            let span = left.span.to(right.span);
            left = Expr {
                kind: ExprKind::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            };
        }
        left
    }

    fn prefix(&mut self) -> Expr {
        let t = self.bump();
        let span = t.span;
        let kind = match t.tok {
            Tok::Number(n) => ExprKind::Number(n),
            Tok::Text(s) => ExprKind::Text(s),
            Tok::Minus | Tok::Plus => {
                let op = if t.tok == Tok::Minus {
                    UnaryOp::Neg
                } else {
                    UnaryOp::Plus
                };
                let e = self.expr(8);
                return Expr {
                    span: span.to(e.span),
                    kind: ExprKind::Unary {
                        op,
                        expr: Box::new(e),
                    },
                };
            }
            Tok::LParen => {
                let e = self.expr(0);
                self.expect_close(span);
                return e;
            }
            Tok::Bracket(name) => ExprKind::Ref { table: None, name },
            Tok::LBrace => {
                let mut items = Vec::new();
                if !matches!(self.peek(), Tok::RBrace) {
                    loop {
                        items.push(self.expr(0));
                        if matches!(self.peek(), Tok::Comma) {
                            self.bump();
                            continue;
                        }
                        break;
                    }
                }
                let end = if matches!(self.peek(), Tok::RBrace) {
                    self.bump().span
                } else {
                    let at = self.span();
                    let found = describe(self.peek());
                    self.error(
                        at,
                        &format!("expected \"}}\" to close the list, found {found}"),
                    );
                    at
                };
                return Expr {
                    kind: ExprKind::List(items),
                    span: span.to(end),
                };
            }
            Tok::Quoted(table) => return self.after_table(table, span),
            Tok::Ident(name) => {
                let upper = name.to_ascii_uppercase();
                if matches!(self.peek(), Tok::LParen) {
                    return self.call(name, span);
                }
                match upper.as_str() {
                    "VAR" => return self.var(span),
                    "TRUE" => ExprKind::Bool(true),
                    "FALSE" => ExprKind::Bool(false),
                    "NOT" => {
                        let e = self.expr(3);
                        return Expr {
                            span: span.to(e.span),
                            kind: ExprKind::Unary {
                                op: UnaryOp::Not,
                                expr: Box::new(e),
                            },
                        };
                    }
                    _ => return self.after_table(name, span),
                }
            }
            other => {
                self.error(
                    span,
                    &format!("expected a value, found {}", describe(&other)),
                );
                // Leave closing tokens for the caller (`bump` never moves past Eof).
                if matches!(other, Tok::RParen | Tok::Comma) {
                    self.pos -= 1;
                }
                ExprKind::Error
            }
        };
        Expr { kind, span }
    }

    /// `VAR name = value` (more VARs) `RETURN body`; called after the `VAR` keyword.
    fn var(&mut self, start: Span) -> Expr {
        let (name, name_span) = match self.peek().clone() {
            Tok::Ident(n) => (n, self.bump().span),
            other => {
                let at = self.span();
                self.error(
                    at,
                    &format!(
                        "expected a variable name after VAR, found {}",
                        describe(&other)
                    ),
                );
                return Expr {
                    kind: ExprKind::Error,
                    span: start.to(at),
                };
            }
        };
        if matches!(self.peek(), Tok::Eq) {
            self.bump();
        } else {
            let at = self.span();
            let found = describe(self.peek());
            self.error(
                at,
                &format!("expected \"=\" after VAR {name}, found {found}"),
            );
        }
        let value = self.expr(0);
        let body = match self.peek().clone() {
            Tok::Ident(k) if k.eq_ignore_ascii_case("VAR") => {
                let s = self.bump().span;
                self.var(s)
            }
            Tok::Ident(k) if k.eq_ignore_ascii_case("RETURN") => {
                self.bump();
                self.expr(0)
            }
            other => {
                let at = self.span();
                self.error(
                    at,
                    &format!("expected RETURN or another VAR, found {}", describe(&other)),
                );
                Expr {
                    kind: ExprKind::Error,
                    span: at,
                }
            }
        };
        Expr {
            span: start.to(body.span),
            kind: ExprKind::Var {
                name,
                name_span,
                value: Box::new(value),
                body: Box::new(body),
            },
        }
    }

    /// A table name, optionally followed by `[Column]`.
    fn after_table(&mut self, table: String, span: Span) -> Expr {
        if let Tok::Bracket(name) = self.peek().clone() {
            let end = self.bump().span;
            return Expr {
                kind: ExprKind::Ref {
                    table: Some(table),
                    name,
                },
                span: span.to(end),
            };
        }
        Expr {
            kind: ExprKind::Table(table),
            span,
        }
    }

    fn call(&mut self, name: String, start: Span) -> Expr {
        let open = self.bump().span;
        let mut args = Vec::new();
        if !matches!(self.peek(), Tok::RParen) {
            loop {
                if matches!(self.peek(), Tok::Comma) {
                    // An omitted optional argument, as in RANKX ( T, e, , ASC ): the
                    // binder decides whether the function allows it.
                    let s = self.span();
                    args.push(Expr {
                        kind: ExprKind::Omitted,
                        span: s,
                    });
                } else {
                    args.push(self.expr(0));
                }
                if matches!(self.peek(), Tok::Comma) {
                    self.bump();
                    continue;
                }
                break;
            }
        }
        let end = self.expect_close(open);
        Expr {
            kind: ExprKind::Call { name, args },
            span: start.to(end),
        }
    }

    fn expect_close(&mut self, open: Span) -> Span {
        if matches!(self.peek(), Tok::RParen) {
            return self.bump().span;
        }
        let at = self.span();
        let found = describe(self.peek());
        self.error(
            at,
            &format!("expected \")\" to close the \"(\" here, found {found}"),
        );
        if let Some(d) = self.diags.last_mut() {
            d.related = Some(open);
        }
        at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Renders a tree compactly for assertions.
    fn show(e: &Expr) -> String {
        match &e.kind {
            ExprKind::Number(n) => n.to_string(),
            ExprKind::Text(s) => format!("{s:?}"),
            ExprKind::Bool(b) => b.to_string(),
            ExprKind::Ref { table, name } => format!("{}[{name}]", table.as_deref().unwrap_or("")),
            ExprKind::Table(t) => format!("table {t}"),
            ExprKind::Call { name, args } => format!(
                "{name}({})",
                args.iter().map(show).collect::<Vec<_>>().join(", ")
            ),
            ExprKind::Unary { op, expr } => format!("({op:?} {})", show(expr)),
            ExprKind::Binary { op, left, right } => {
                format!("({} {op:?} {})", show(left), show(right))
            }
            ExprKind::Var {
                name, value, body, ..
            } => format!("(var {name} = {} in {})", show(value), show(body)),
            ExprKind::List(items) => format!(
                "{{{}}}",
                items.iter().map(show).collect::<Vec<_>>().join(", ")
            ),
            ExprKind::Error => "!".into(),
            ExprKind::Omitted => "_".into(),
        }
    }

    fn ok(src: &str) -> String {
        let p = parse(src);
        assert!(p.diagnostics.is_empty(), "{src}: {:?}", p.diagnostics);
        show(&p.expr)
    }

    #[test]
    fn precedence() {
        assert_eq!(ok("1 + 2 * 3 ^ 2 ^ 2"), "(1 Add (2 Mul (3 Pow (2 Pow 2))))");
        assert_eq!(ok("-2 ^ 2"), "(Neg (2 Pow 2))");
        assert_eq!(ok("1 - 2 - 3"), "((1 Sub 2) Sub 3)");
        assert_eq!(
            ok("a[x] & \"y\" = \"z\" || NOT TRUE && FALSE"),
            "(((a[x] Concat \"y\") Eq \"z\") Or ((Not true) And false))"
        );
        assert_eq!(ok("(1 + 2) * 3"), "((1 Add 2) Mul 3)");
    }

    #[test]
    fn references_and_calls() {
        assert_eq!(
            ok("DIVIDE ( SUM ( 'Sales Lines'[Amount] ) - [Cost], COUNTROWS ( Sales ), 0 )"),
            "DIVIDE((SUM(Sales Lines[Amount]) Sub [Cost]), COUNTROWS(table Sales), 0)"
        );
        assert_eq!(ok("BLANK()"), "BLANK()");
        assert_eq!(ok("Sales[Qty] * 2"), "(Sales[Qty] Mul 2)");
    }

    #[test]
    fn variables_lists_and_in() {
        assert_eq!(
            ok("VAR a = 1 VAR b = a + 1 RETURN a * b"),
            "(var a = 1 in (var b = (table a Add 1) in (table a Mul table b)))"
        );
        assert_eq!(
            ok("Product[Color] IN { \"Red\", \"Blue\" }"),
            "(Product[Color] In {\"Red\", \"Blue\"})"
        );
        assert!(
            parse("VAR a = 1 a").diagnostics[0]
                .message
                .contains("expected RETURN")
        );
    }

    #[test]
    fn errors_are_located_and_parsing_continues() {
        let p = parse("SUM ( Sales[Amount] ");
        assert_eq!(p.diagnostics.len(), 1);
        assert!(p.diagnostics[0].message.contains("expected \")\""));
        assert_eq!(p.diagnostics[0].span.start, 20);

        let p = parse("1 + * 2");
        assert!(p.diagnostics[0].message.contains("expected a value"));

        // An omitted argument parses; the binder decides whether it's allowed.
        let p = parse("IF ( [x] > 1, , 2 )");
        assert!(p.diagnostics.is_empty());

        let p = parse("1 2");
        assert!(p.diagnostics[0].message.contains("after the end"));

        assert_eq!(parse("").diagnostics[0].message, "the formula is empty");
        assert!(
            parse("[Amount").diagnostics[0]
                .message
                .contains("unclosed [")
        );
    }
}
