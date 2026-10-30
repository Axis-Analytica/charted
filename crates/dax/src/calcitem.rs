//! Calculation items: an item's expression applied to a measure, by replacing
//! SELECTEDMEASURE () with a reference to it (and SELECTEDMEASURENAME,
//! ISSELECTEDMEASURE and SELECTEDMEASUREFORMATSTRING with what they give for it).

use crate::lexer::{Tok, lex};

/// The measure an item is applied to.
pub struct Selected<'a> {
    /// A DAX reference to it (`'Sales'[Total Sales]`), or an expression in parentheses.
    pub reference: &'a str,
    pub name: &'a str,
    pub format_string: Option<&'a str>,
}

/// The functions that stand for the measure a calculation item is applied to.
pub const FUNCTIONS: &[&str] = &[
    "SELECTEDMEASURE",
    "SELECTEDMEASURENAME",
    "SELECTEDMEASUREFORMATSTRING",
    "ISSELECTEDMEASURE",
];

/// A DAX string literal.
pub fn text(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// `src` (an item's expression) applied to `m`.
pub fn apply(src: &str, m: &Selected) -> String {
    let toks = lex(src);
    let mut out = String::new();
    let mut at = 0;
    let mut i = 0;
    while i < toks.len() {
        let Tok::Ident(name) = &toks[i].tok else {
            i += 1;
            continue;
        };
        let upper = name.to_ascii_uppercase();
        if !FUNCTIONS.contains(&upper.as_str())
            || !matches!(toks.get(i + 1).map(|t| &t.tok), Some(Tok::LParen))
        {
            i += 1;
            continue;
        }
        // The matching parenthesis, and the measures named in between.
        let mut depth = 0;
        let mut j = i + 1;
        let mut named = false;
        while j < toks.len() {
            match &toks[j].tok {
                Tok::LParen => depth += 1,
                Tok::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                Tok::Bracket(b) if b.eq_ignore_ascii_case(m.name) => named = true,
                Tok::Eof => break,
                _ => {}
            }
            j += 1;
        }
        if j >= toks.len() || !matches!(toks[j].tok, Tok::RParen) {
            break;
        }
        let with = match upper.as_str() {
            "SELECTEDMEASURE" => m.reference.to_string(),
            "SELECTEDMEASURENAME" => text(m.name),
            "SELECTEDMEASUREFORMATSTRING" => m
                .format_string
                .map(text)
                .unwrap_or_else(|| "BLANK ( )".into()),
            _ => if named { "TRUE ( )" } else { "FALSE ( )" }.to_string(),
        };
        out.push_str(&src[at..toks[i].span.start]);
        out.push_str(&with);
        at = toks[j].span.end;
        i = j + 1;
    }
    out.push_str(&src[at..]);
    out
}
