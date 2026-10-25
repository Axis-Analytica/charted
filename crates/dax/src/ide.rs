//! Editor services over a formula and the model: completion, signature help, hover and
//! formatting. Offsets are byte offsets into the formula text.

use crate::Span;
use crate::catalog::{self, FunctionDoc};
use crate::lexer::{Tok, Token, lex, lex_with_comments};
use charted_model::{ColumnKind, Model, Table};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CompletionKind {
    Function,
    Table,
    Column,
    Measure,
    Variable,
    Keyword,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Completion {
    pub label: String,
    pub kind: CompletionKind,
    /// Shown next to the label: a signature, a table, a type.
    pub detail: String,
    pub documentation: Option<String>,
    pub insert_text: String,
    /// The text the completion replaces.
    pub replace: Span,
    /// Orders the list: lower first.
    pub sort: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureHelp {
    pub label: String,
    pub documentation: String,
    pub params: Vec<String>,
    pub active: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hover {
    /// Markdown.
    pub contents: String,
    pub span: Span,
}

const KEYWORDS: &[&str] = &["VAR", "RETURN", "TRUE", "FALSE", "IN", "NOT"];

/// DAX quotes a table name unless it is a plain identifier.
pub fn table_ref(name: &str) -> String {
    let plain = !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && !KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(name))
        && catalog::function(name).is_none();
    if plain {
        name.to_string()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}

fn bracket(name: &str) -> String {
    format!("[{}]", name.replace(']', "]]"))
}

fn type_name(c: &charted_model::Column) -> String {
    format!("{:?}", c.data_type)
}

fn find_table<'a>(model: &'a Model, name: &str) -> Option<&'a Table> {
    model
        .tables
        .iter()
        .find(|t| t.name.eq_ignore_ascii_case(name))
}

/// The table token right before `i` (as in `Sales[` or `'Sales Lines'[`), if any.
fn table_before(text: &str, i: usize) -> Option<String> {
    let before = &text[..i];
    if let Some(stripped) = before.strip_suffix('\'') {
        // A quoted name: find its opening quote (doubled quotes are escapes).
        let mut j = stripped.len();
        let b = stripped.as_bytes();
        while j > 0 {
            j -= 1;
            if b[j] == b'\'' {
                if j > 0 && b[j - 1] == b'\'' {
                    j -= 1;
                    continue;
                }
                return Some(stripped[j + 1..].replace("''", "'"));
            }
        }
        return None;
    }
    let word: String = before
        .chars()
        .rev()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if word.is_empty() {
        None
    } else {
        Some(word.chars().rev().collect())
    }
}

/// VAR names declared before `offset`.
fn variables(tokens: &[Token], offset: usize) -> Vec<String> {
    let mut out = Vec::new();
    for w in tokens.windows(2) {
        if w[1].span.end > offset {
            break;
        }
        if let (Tok::Ident(k), Tok::Ident(name)) = (&w[0].tok, &w[1].tok)
            && k.eq_ignore_ascii_case("VAR")
        {
            out.push(name.clone());
        }
    }
    out
}

fn function_completion(f: &FunctionDoc, replace: Span) -> Completion {
    Completion {
        label: f.name.to_string(),
        kind: CompletionKind::Function,
        detail: f.signature(),
        documentation: Some(if f.supported {
            f.description.to_string()
        } else {
            format!(
                "{} (charted does not evaluate this function yet.)",
                f.description
            )
        }),
        insert_text: format!("{} ( ", f.name),
        replace,
        sort: format!("{}{}", if f.supported { "2" } else { "4" }, f.name),
    }
}

fn column_items(t: &Table, qualified: bool, replace: Span, out: &mut Vec<Completion>) {
    for c in t.columns.iter().filter(|c| !c.hidden) {
        let label = if qualified {
            format!("{}{}", table_ref(&t.name), bracket(&c.name))
        } else {
            bracket(&c.name)
        };
        let calc = matches!(c.kind, ColumnKind::Calculated { .. });
        out.push(Completion {
            insert_text: label.clone(),
            label,
            kind: CompletionKind::Column,
            detail: format!(
                "{} column in {}{}",
                type_name(c),
                t.name,
                if calc { " (calculated)" } else { "" }
            ),
            documentation: c.description.clone(),
            replace,
            sort: format!("1{}", c.name),
        });
    }
}

fn measure_items(model: &Model, only: Option<&Table>, replace: Span, out: &mut Vec<Completion>) {
    for t in &model.tables {
        if only.is_some_and(|o| o.id != t.id) {
            continue;
        }
        for m in t.measures.iter().filter(|m| !m.hidden) {
            out.push(Completion {
                label: bracket(&m.name),
                kind: CompletionKind::Measure,
                detail: format!("Measure in {}", t.name),
                documentation: Some(format!("```dax\n{}\n```", m.expression)),
                insert_text: bracket(&m.name),
                replace,
                sort: format!("0{}", m.name),
            });
        }
    }
}

/// Completions at `offset`. `home` is the table a calculated column belongs to (its
/// columns can be named without the table).
pub fn complete(model: &Model, text: &str, offset: usize, home: Option<&str>) -> Vec<Completion> {
    let offset = offset.min(text.len());
    let tokens = lex(text);
    let mut out = Vec::new();
    let home = home.and_then(|h| model.table(h));

    // Inside a string literal: nothing to offer.
    if tokens
        .iter()
        .any(|t| matches!(t.tok, Tok::Text(_)) && t.span.start < offset && offset < t.span.end)
    {
        return out;
    }
    let before = &text[..offset];
    // Inside `[…`: columns and measures.
    let open_bracket = before.rfind('[').filter(|&i| !before[i..].contains(']'));
    if let Some(i) = open_bracket {
        let end = if text[offset..].starts_with(']') {
            offset + 1
        } else {
            offset
        };
        let replace = Span { start: i, end };
        match table_before(text, i).and_then(|n| find_table(model, &n)) {
            Some(t) => {
                column_items(t, false, replace, &mut out);
                measure_items(model, Some(t), replace, &mut out);
            }
            None => {
                measure_items(model, None, replace, &mut out);
                if let Some(h) = home {
                    column_items(h, false, replace, &mut out);
                }
            }
        }
        let prefix = before[i + 1..].to_lowercase();
        out.retain(|c| {
            c.label
                .to_lowercase()
                .trim_start_matches('[')
                .contains(&prefix)
        });
        out.sort_by(|a, b| a.sort.cmp(&b.sort));
        return out;
    }
    // Inside `'…`: table names.
    let quotes = before.matches('\'').count();
    if quotes % 2 == 1 {
        let i = before.rfind('\'').unwrap_or(0);
        let prefix = before[i + 1..].to_lowercase();
        let end = if text[offset..].starts_with('\'') {
            offset + 1
        } else {
            offset
        };
        for t in model
            .tables
            .iter()
            .filter(|t| !t.hidden && t.name.to_lowercase().contains(&prefix))
        {
            out.push(Completion {
                label: format!("'{}'", t.name),
                kind: CompletionKind::Table,
                detail: "Table".into(),
                documentation: None,
                insert_text: format!("'{}'", t.name.replace('\'', "''")),
                replace: Span { start: i, end },
                sort: format!("1{}", t.name),
            });
        }
        return out;
    }
    // A word: functions, tables, Table[Column], variables, keywords.
    let word_start = before
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || *c == '_' || *c == '.')
        .last()
        .map(|(i, _)| i)
        .unwrap_or(offset);
    let prefix = before[word_start..].to_lowercase();
    let replace = Span {
        start: word_start,
        end: offset,
    };
    let matches = |s: &str| prefix.is_empty() || s.to_lowercase().starts_with(&prefix);
    for v in variables(&tokens, word_start) {
        if matches(&v) {
            out.push(Completion {
                label: v.clone(),
                kind: CompletionKind::Variable,
                detail: "Variable".into(),
                documentation: None,
                insert_text: v.clone(),
                replace,
                sort: format!("0{v}"),
            });
        }
    }
    for f in catalog::functions() {
        if matches(f.name) {
            out.push(function_completion(&f, replace));
        }
    }
    for t in model.tables.iter().filter(|t| !t.hidden) {
        if matches(&t.name) {
            out.push(Completion {
                label: t.name.clone(),
                kind: CompletionKind::Table,
                detail: "Table".into(),
                documentation: t.description.clone(),
                insert_text: table_ref(&t.name),
                replace,
                sort: format!("1{}", t.name),
            });
            // Typing a table's name offers its columns too, as Power BI does.
            if !prefix.is_empty() {
                column_items(t, true, replace, &mut out);
            }
        }
    }
    if let Some(h) = home
        && !prefix.is_empty()
    {
        let mut cols = Vec::new();
        column_items(h, false, replace, &mut cols);
        out.extend(
            cols.into_iter()
                .filter(|c| c.label[1..].to_lowercase().starts_with(&prefix)),
        );
    }
    if !prefix.is_empty() {
        let mut ms = Vec::new();
        measure_items(model, None, replace, &mut ms);
        out.extend(
            ms.into_iter()
                .filter(|c| c.label[1..].to_lowercase().starts_with(&prefix)),
        );
    }
    for k in KEYWORDS {
        if matches(k) && catalog::function(k).is_none() {
            out.push(Completion {
                label: k.to_string(),
                kind: CompletionKind::Keyword,
                detail: "Keyword".into(),
                documentation: None,
                insert_text: format!("{k} "),
                replace,
                sort: format!("3{k}"),
            });
        }
    }
    out.sort_by(|a, b| a.sort.cmp(&b.sort));
    out
}

/// The function call around `offset` and which argument the cursor is in.
pub fn signature(text: &str, offset: usize) -> Option<SignatureHelp> {
    let tokens = lex(text);
    let mut stack: Vec<(Option<String>, usize)> = Vec::new();
    let mut prev: Option<&Tok> = None;
    for t in &tokens {
        if t.span.start >= offset || t.tok == Tok::Eof {
            break;
        }
        match &t.tok {
            Tok::LParen => stack.push((
                match prev {
                    Some(Tok::Ident(n)) => Some(n.clone()),
                    _ => None,
                },
                0,
            )),
            Tok::LBrace => stack.push((None, 0)),
            Tok::RParen | Tok::RBrace => {
                stack.pop();
            }
            Tok::Comma => {
                if let Some(top) = stack.last_mut() {
                    top.1 += 1;
                }
            }
            _ => {}
        }
        prev = Some(&t.tok);
    }
    let (name, arg) = stack.iter().rev().find(|(n, _)| n.is_some())?;
    let f = catalog::function(name.as_deref()?)?;
    let n = f.params.len();
    let active = if n == 0 {
        0
    } else if *arg < n {
        *arg
    } else {
        // Past the end: stay on the repeating parameter (the last one written `…`).
        f.params
            .iter()
            .rposition(|p| p.contains('…'))
            .unwrap_or(n - 1)
    };
    Some(SignatureHelp {
        label: f.signature(),
        documentation: f.description.to_string(),
        params: f.params.iter().map(|p| p.to_string()).collect(),
        active,
    })
}

/// What the token at `offset` refers to.
pub fn hover(model: &Model, text: &str, offset: usize, home: Option<&str>) -> Option<Hover> {
    let tokens = lex(text);
    let i = tokens
        .iter()
        .position(|t| t.span.start <= offset && offset < t.span.end.max(t.span.start + 1))?;
    let t = &tokens[i];
    let next_is_paren = tokens.get(i + 1).is_some_and(|n| n.tok == Tok::LParen);
    let table_of = |tok: &Tok| match tok {
        Tok::Ident(n) | Tok::Quoted(n) => find_table(model, n),
        _ => None,
    };
    let contents = match &t.tok {
        Tok::Ident(n) if next_is_paren => {
            let f = catalog::function(n)?;
            let note = if f.supported {
                ""
            } else {
                "\n\n*charted does not evaluate this function yet.*"
            };
            format!("```dax\n{}\n```\n{}{}", f.signature(), f.description, note)
        }
        Tok::Ident(_) | Tok::Quoted(_) if table_of(&t.tok).is_some() => {
            let tb = table_of(&t.tok)?;
            format!(
                "**{}** (table) · {} columns, {} measures",
                tb.name,
                tb.columns.len(),
                tb.measures.len()
            )
        }
        Tok::Bracket(name) => {
            let table = if i > 0 && tokens[i - 1].span.end == t.span.start {
                table_of(&tokens[i - 1].tok)
            } else {
                None
            };
            fn column<'m>(
                tb: &'m Table,
                name: &str,
            ) -> Option<(&'m Table, &'m charted_model::Column)> {
                tb.columns
                    .iter()
                    .find(|c| c.name.eq_ignore_ascii_case(name))
                    .map(|c| (tb, c))
            }
            let measure = model
                .tables
                .iter()
                .filter(|tb| table.is_none_or(|x| x.id == tb.id))
                .find_map(|tb| {
                    tb.measures
                        .iter()
                        .find(|m| m.name.eq_ignore_ascii_case(name))
                        .map(|m| (tb, m))
                });
            if let Some((tb, c)) = table.and_then(|t| column(t, name)).or_else(|| {
                home.and_then(|h| model.table(h))
                    .and_then(|t| column(t, name))
            }) {
                let extra = match &c.kind {
                    ColumnKind::Calculated { expression, .. } => {
                        format!("\n```dax\n{expression}\n```")
                    }
                    ColumnKind::Data { .. } => String::new(),
                };
                format!(
                    "**{}[{}]** · {} column{}{}",
                    tb.name,
                    c.name,
                    type_name(c),
                    c.description
                        .as_ref()
                        .map(|d| format!("\n\n{d}"))
                        .unwrap_or_default(),
                    extra
                )
            } else if let Some((tb, m)) = measure {
                format!(
                    "**[{}]** · measure in {}\n```dax\n{}\n```",
                    m.name, tb.name, m.expression
                )
            } else {
                return None;
            }
        }
        _ => return None,
    };
    Some(Hover {
        contents,
        span: t.span,
    })
}

// ---- Formatting ----------------------------------------------------------------

#[derive(Debug, Clone)]
enum Node {
    Atom {
        text: String,
        tok: Tok,
    },
    Comment(String),
    /// `(…)` or `{…}`, with arguments split at top-level commas.
    Group {
        open: &'static str,
        close: &'static str,
        args: Vec<Vec<Node>>,
    },
}

fn atom_text(src: &str, t: &Token, next: Option<&Token>) -> String {
    let raw = &src[t.span.start..t.span.end];
    match &t.tok {
        Tok::Ident(n) => {
            let upper = n.to_uppercase();
            if next.is_some_and(|x| x.tok == Tok::LParen) || KEYWORDS.contains(&upper.as_str()) {
                upper
            } else {
                raw.to_string()
            }
        }
        _ => raw.to_string(),
    }
}

/// Parses tokens (with comments interleaved by position) into nodes until a closer.
fn nodes(
    src: &str,
    items: &[(Span, Option<&Token>)],
    i: &mut usize,
    close: Option<Tok>,
) -> Vec<Vec<Node>> {
    let mut args: Vec<Vec<Node>> = vec![Vec::new()];
    while *i < items.len() {
        let (span, tok) = items[*i];
        let Some(t) = tok else {
            args.last_mut().unwrap().push(Node::Comment(
                src[span.start..span.end].trim_end().to_string(),
            ));
            *i += 1;
            continue;
        };
        if close.as_ref() == Some(&t.tok) {
            *i += 1;
            return args;
        }
        *i += 1;
        match &t.tok {
            Tok::Eof => break,
            Tok::Comma if close.is_some() => args.push(Vec::new()),
            Tok::LParen | Tok::LBrace => {
                let (open, closer, cl) = if t.tok == Tok::LParen {
                    ("(", ")", Tok::RParen)
                } else {
                    ("{", "}", Tok::RBrace)
                };
                let inner = nodes(src, items, i, Some(cl));
                let inner = if inner.len() == 1 && inner[0].is_empty() {
                    Vec::new()
                } else {
                    inner
                };
                args.last_mut().unwrap().push(Node::Group {
                    open,
                    close: closer,
                    args: inner,
                });
            }
            _ => {
                let next = items[*i..].iter().find_map(|(_, t)| *t);
                args.last_mut().unwrap().push(Node::Atom {
                    text: atom_text(src, t, next),
                    tok: t.tok.clone(),
                });
            }
        }
    }
    args
}

fn is_keyword(n: &Node, k: &str) -> bool {
    matches!(n, Node::Atom { tok: Tok::Ident(x), .. } if x.eq_ignore_ascii_case(k))
}

fn has_comment(ns: &[Node]) -> bool {
    ns.iter().any(|n| match n {
        Node::Comment(_) => true,
        Node::Group { args, .. } => args.iter().any(|a| has_comment(a)),
        Node::Atom { .. } => false,
    })
}

fn has_var(ns: &[Node]) -> bool {
    ns.iter().any(|n| match n {
        Node::Group { args, .. } => args.iter().any(|a| has_var(a)),
        n => is_keyword(n, "VAR"),
    })
}

/// One line, spaced the DAX Formatter way: `SUM ( Sales[Amount] )`.
fn flat(ns: &[Node]) -> String {
    let mut out = String::new();
    let mut prev: Option<&Node> = None;
    for n in ns {
        let piece = match n {
            Node::Atom { text, .. } => text.clone(),
            Node::Comment(c) => c.clone(),
            Node::Group { open, close, args } => {
                if args.is_empty() {
                    format!("{open}{close}")
                } else {
                    format!(
                        "{open} {} {close}",
                        args.iter().map(|a| flat(a)).collect::<Vec<_>>().join(", ")
                    )
                }
            }
        };
        let glue = match (prev, n) {
            (None, _) => "",
            // Table[Column]: no space.
            (
                Some(Node::Atom {
                    tok: Tok::Ident(_) | Tok::Quoted(_),
                    ..
                }),
                Node::Atom {
                    tok: Tok::Bracket(_),
                    ..
                },
            ) => "",
            // A unary sign right after an operator or at the start of an argument.
            (Some(Node::Atom { tok: p, .. }), _)
                if matches!(p, Tok::Minus | Tok::Plus)
                    && out.trim_end().len() == out.len()
                    && unary_context(&out) =>
            {
                ""
            }
            _ => " ",
        };
        out.push_str(glue);
        out.push_str(&piece);
        prev = Some(n);
    }
    out
}

/// Whether a trailing `-`/`+` in `out` is a sign rather than an operator.
fn unary_context(out: &str) -> bool {
    let t = out.trim_end_matches(['-', '+']).trim_end();
    t.is_empty()
        || t.ends_with(['(', ',', '{', '=', '*', '/', '^', '&', '<', '>', '|'])
        || t.ends_with(" -")
        || t.ends_with(" +")
        || t.to_uppercase().ends_with("RETURN")
}

const WIDTH: usize = 80;

fn render(ns: &[Node], indent: usize, out: &mut String) {
    let pad = |n: usize| "    ".repeat(n);
    // VAR … RETURN: one declaration per line, the result indented under RETURN.
    if ns.first().is_some_and(|n| is_keyword(n, "VAR"))
        || ns.iter().any(|n| is_keyword(n, "RETURN"))
    {
        let mut parts: Vec<(String, Vec<Node>)> = Vec::new();
        let mut cur: Option<(String, Vec<Node>)> = None;
        for n in ns {
            if is_keyword(n, "VAR") || is_keyword(n, "RETURN") {
                if let Some(c) = cur.take() {
                    parts.push(c);
                }
                let Node::Atom { text, .. } = n else {
                    unreachable!()
                };
                cur = Some((text.clone(), Vec::new()));
            } else if let Some(c) = cur.as_mut() {
                c.1.push(n.clone());
            } else {
                cur = Some((String::new(), vec![n.clone()]));
            }
        }
        parts.extend(cur);
        for (i, (kw, body)) in parts.iter().enumerate() {
            if i > 0 {
                out.push('\n');
                out.push_str(&pad(indent));
            }
            if kw == "RETURN" {
                out.push_str("RETURN\n");
                out.push_str(&pad(indent + 1));
                render(body, indent + 1, out);
            } else if kw.is_empty() {
                render(body, indent, out);
            } else {
                out.push_str(kw);
                out.push(' ');
                render(body, indent, out);
            }
        }
        return;
    }
    let line = flat(ns);
    if !has_comment(ns) && !has_var(ns) && indent * 4 + line.len() <= WIDTH {
        out.push_str(&line);
        return;
    }
    // Too long: keep everything up to each group on the line, break inside groups.
    let mut prev: Option<&Node> = None;
    for n in ns {
        if let Some(p) = prev {
            let tight = matches!(
                (p, n),
                (
                    Node::Atom {
                        tok: Tok::Ident(_) | Tok::Quoted(_),
                        ..
                    },
                    Node::Atom {
                        tok: Tok::Bracket(_),
                        ..
                    }
                )
            );
            if matches!(p, Node::Comment(_)) {
                out.push('\n');
                out.push_str(&pad(indent));
            } else if !tight {
                out.push(' ');
            }
        }
        match n {
            Node::Atom { text, .. } => out.push_str(text),
            Node::Comment(c) => out.push_str(c),
            Node::Group { open, close, args } => {
                let one = format!(
                    "{open} {} {close}",
                    args.iter().map(|a| flat(a)).collect::<Vec<_>>().join(", ")
                );
                let col = out.rsplit('\n').next().map(str::len).unwrap_or(0);
                if args.is_empty() {
                    out.push_str(&format!("{open}{close}"));
                } else if !args.iter().any(|a| has_comment(a) || has_var(a))
                    && col + one.len() <= WIDTH
                {
                    out.push_str(&one);
                } else {
                    out.push_str(open);
                    for (i, a) in args.iter().enumerate() {
                        out.push('\n');
                        out.push_str(&pad(indent + 1));
                        render(a, indent + 1, out);
                        if i + 1 < args.len() {
                            out.push(',');
                        }
                    }
                    out.push('\n');
                    out.push_str(&pad(indent));
                    out.push_str(close);
                }
            }
        }
        prev = Some(n);
    }
}

/// Formats a DAX expression: upper-case functions and keywords, spaces inside
/// parentheses, one argument per line when a call does not fit on one line, and one
/// line per VAR. Comments are kept. Text that does not lex cleanly is returned as is.
pub fn format(src: &str) -> String {
    let (tokens, comments) = lex_with_comments(src);
    if tokens.iter().any(|t| matches!(t.tok, Tok::Unknown(_))) {
        return src.to_string();
    }
    let mut items: Vec<(Span, Option<&Token>)> = tokens.iter().map(|t| (t.span, Some(t))).collect();
    items.extend(comments.into_iter().map(|c| (c, None)));
    items.sort_by_key(|(s, t)| (s.start, t.is_some()));
    let mut i = 0;
    let top = nodes(src, &items, &mut i, None);
    let mut out = String::new();
    // Stray top-level commas are not valid DAX; keep them rather than lose text.
    for (k, part) in top.iter().enumerate() {
        if k > 0 {
            out.push_str(", ");
        }
        render(part, 0, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_like_dax_formatter() {
        assert_eq!(format("sum(Sales[Amount])"), "SUM ( Sales[Amount] )");
        assert_eq!(format("divide([a]-[b],[a])"), "DIVIDE ( [a] - [b], [a] )");
        assert_eq!(format("-1 * -[x]"), "-1 * -[x]");
        assert_eq!(format("blank()"), "BLANK ()");
        assert_eq!(format("x in {1,2}"), "x IN { 1, 2 }");
        assert_eq!(
            format("var r = sum(Sales[Amount]) var c = sum(Sales[Cost]) return divide(r-c,r)"),
            "VAR r = SUM ( Sales[Amount] )\nVAR c = SUM ( Sales[Cost] )\nRETURN\n    DIVIDE ( r - c, r )"
        );
        let long = format(
            "CALCULATE(SUM(Sales[Amount]), USERELATIONSHIP(Sales[ShipDateKey], 'Date'[DateKey]), ALL('Date'))",
        );
        assert_eq!(
            long,
            "CALCULATE (\n    SUM ( Sales[Amount] ),\n    USERELATIONSHIP ( Sales[ShipDateKey], 'Date'[DateKey] ),\n    ALL ( 'Date' )\n)"
        );
        // Comments survive.
        assert!(format("SUM(Sales[Amount]) // total").contains("// total"));
        // Formatting is stable.
        assert_eq!(format(&long), long);
    }

    #[test]
    fn signature_tracks_the_argument() {
        let s = signature("DIVIDE ( [a], ", 14).unwrap();
        assert_eq!(
            s.label,
            "DIVIDE ( Numerator, Denominator, [AlternateResult] )"
        );
        assert_eq!(s.active, 1);
        let s = signature("CALCULATE ( [x], A, B, C", 23).unwrap();
        assert_eq!(s.active, 1, "stays on the repeating filter parameter");
        assert!(signature("SUM ( Sales[Amount] ) + ", 24).is_none());
    }
}
