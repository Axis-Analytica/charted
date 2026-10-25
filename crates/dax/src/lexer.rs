//! Splits DAX text into tokens. Never fails: unknown characters become `Unknown`
//! tokens that the parser reports.

use crate::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Number(f64),
    /// A string literal, unescaped.
    Text(String),
    /// A bare name: a function, table or keyword (TRUE, FALSE, NOT, IN, VAR, RETURN).
    Ident(String),
    /// A quoted table name: `'Sales Lines'`, unescaped.
    Quoted(String),
    /// A bracketed column or measure name: `[Amount]`, unescaped.
    Bracket(String),
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    Amp,
    Eq,
    EqEq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    /// Something DAX does not allow here, or an unterminated literal.
    Unknown(String),
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

pub fn lex(src: &str) -> Vec<Token> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let start = i;
        // Whitespace and comments.
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if (c == b'/' && b.get(i + 1) == Some(&b'/')) || (c == b'-' && b.get(i + 1) == Some(&b'-'))
        {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i += 2;
            while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                i += 1;
            }
            i = (i + 2).min(b.len());
            continue;
        }
        let tok = match c {
            b'"' | b'\'' | b'[' => {
                let close = if c == b'[' { b']' } else { c };
                i += 1;
                let mut text = String::new();
                let mut closed = false;
                while i < b.len() {
                    if b[i] == close {
                        // A doubled closing character is an escaped one.
                        if b.get(i + 1) == Some(&close) {
                            text.push(close as char);
                            i += 2;
                            continue;
                        }
                        i += 1;
                        closed = true;
                        break;
                    }
                    let ch = src[i..].chars().next().unwrap_or(' ');
                    text.push(ch);
                    i += ch.len_utf8();
                }
                if !closed {
                    Tok::Unknown(src[start..i].to_string())
                } else {
                    match c {
                        b'"' => Tok::Text(text),
                        b'\'' => Tok::Quoted(text),
                        _ => Tok::Bracket(text),
                    }
                }
            }
            b'0'..=b'9' | b'.' if c != b'.' || b.get(i + 1).is_some_and(u8::is_ascii_digit) => {
                while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                    i += 1;
                }
                if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
                    let mut j = i + 1;
                    if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
                        j += 1;
                    }
                    if j < b.len() && b[j].is_ascii_digit() {
                        i = j;
                        while i < b.len() && b[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                }
                let text = &src[start..i];
                text.parse::<f64>()
                    .map(Tok::Number)
                    .unwrap_or_else(|_| Tok::Unknown(text.to_string()))
            }
            _ if c.is_ascii_alphabetic() || c == b'_' || c >= 0x80 => {
                while i < b.len() {
                    let ch = src[i..].chars().next().unwrap_or(' ');
                    if ch.is_alphanumeric() || ch == '_' || ch == '.' {
                        i += ch.len_utf8();
                    } else {
                        break;
                    }
                }
                Tok::Ident(src[start..i].to_string())
            }
            _ => {
                let two = b.get(i + 1).copied();
                let (t, n) = match (c, two) {
                    (b'=', Some(b'=')) => (Tok::EqEq, 2),
                    (b'<', Some(b'>')) => (Tok::Ne, 2),
                    (b'<', Some(b'=')) => (Tok::Le, 2),
                    (b'>', Some(b'=')) => (Tok::Ge, 2),
                    (b'&', Some(b'&')) => (Tok::AndAnd, 2),
                    (b'|', Some(b'|')) => (Tok::OrOr, 2),
                    (b'(', _) => (Tok::LParen, 1),
                    (b')', _) => (Tok::RParen, 1),
                    (b'{', _) => (Tok::LBrace, 1),
                    (b'}', _) => (Tok::RBrace, 1),
                    // Some locales separate arguments with semicolons.
                    (b',', _) | (b';', _) => (Tok::Comma, 1),
                    (b'+', _) => (Tok::Plus, 1),
                    (b'-', _) => (Tok::Minus, 1),
                    (b'*', _) => (Tok::Star, 1),
                    (b'/', _) => (Tok::Slash, 1),
                    (b'^', _) => (Tok::Caret, 1),
                    (b'&', _) => (Tok::Amp, 1),
                    (b'=', _) => (Tok::Eq, 1),
                    (b'<', _) => (Tok::Lt, 1),
                    (b'>', _) => (Tok::Gt, 1),
                    _ => {
                        let ch = src[i..].chars().next().unwrap_or(' ');
                        (Tok::Unknown(ch.to_string()), ch.len_utf8())
                    }
                };
                i += n;
                t
            }
        };
        out.push(Token {
            tok,
            span: Span { start, end: i },
        });
    }
    out.push(Token {
        tok: Tok::Eof,
        span: Span {
            start: src.len(),
            end: src.len(),
        },
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<Tok> {
        lex(s).into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn names_literals_and_operators() {
        assert_eq!(
            toks("DIVIDE ( 'Sales Lines'[Net]]], [Cost] ) >= 1.5e2 && \"a\"\"b\""),
            vec![
                Tok::Ident("DIVIDE".into()),
                Tok::LParen,
                Tok::Quoted("Sales Lines".into()),
                Tok::Bracket("Net]".into()),
                Tok::Comma,
                Tok::Bracket("Cost".into()),
                Tok::RParen,
                Tok::Ge,
                Tok::Number(150.0),
                Tok::AndAnd,
                Tok::Text("a\"b".into()),
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn comments_and_unterminated_literals() {
        assert_eq!(
            toks("1 // one\n-- two\n/* three */ + 2"),
            vec![Tok::Number(1.0), Tok::Plus, Tok::Number(2.0), Tok::Eof]
        );
        assert!(matches!(toks("[Amount")[0], Tok::Unknown(_)));
    }
}
