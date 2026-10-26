//! Understands the common shapes of Power Query (M) that load a table from a file —
//! CSV, Excel, Parquet and JSON, with promoted headers, type changes, removed,
//! selected and renamed columns, skipped and kept rows and distinct rows — as a
//! charted source and steps. Anything else is reported, not guessed.

use charted_compiler::steps::{Source, Step};
use charted_model::DataType;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
enum M {
    Str(String),
    Num(f64),
    Ident(String),
    Null,
    Bool(bool),
    Call(Box<M>, Vec<M>),
    Record(Vec<(String, M)>),
    List(Vec<M>),
    /// `a & b`
    Concat(Box<M>, Box<M>),
    /// `x{…}` (an item) and `x[…]` (a field).
    Item(Box<M>, Box<M>),
    Field(Box<M>, String),
    /// `type text`, `Int64.Type`…
    Type(String),
    /// `type table [A = text, B = _t]`: the columns and their types.
    TableType(Vec<(String, M)>),
    /// `a + b`, `a - b`, `a * b`, `a / b`.
    Bin(Box<M>, char, Box<M>),
    /// A nested `let … in …` expression.
    Let(Vec<(String, M)>, Box<M>),
    /// Anything this reader doesn't model (an `each` function, operators…).
    Other(String),
}

struct Lexer<'a> {
    s: &'a str,
    i: usize,
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Str(String),
    Num(f64),
    Ident(String),
    Sym(char),
    Eof,
}

impl<'a> Lexer<'a> {
    fn skip(&mut self) {
        loop {
            let rest = &self.s[self.i..];
            let t = rest.trim_start();
            self.i += rest.len() - t.len();
            if t.starts_with("//") {
                self.i += t.find('\n').unwrap_or(t.len());
            } else if t.starts_with("/*") {
                self.i += t.find("*/").map(|e| e + 2).unwrap_or(t.len());
            } else {
                return;
            }
        }
    }

    fn next(&mut self) -> Tok {
        self.skip();
        let rest = &self.s[self.i..];
        let Some(c) = rest.chars().next() else {
            return Tok::Eof;
        };
        if c == '"' || rest.starts_with("#\"") {
            let start = if c == '"' { 1 } else { 2 };
            let mut out = String::new();
            let b = rest.as_bytes();
            let mut j = start;
            while j < b.len() {
                if b[j] == b'"' {
                    if b.get(j + 1) == Some(&b'"') {
                        out.push('"');
                        j += 2;
                        continue;
                    }
                    break;
                }
                let ch = rest[j..].chars().next().unwrap();
                out.push(ch);
                j += ch.len_utf8();
            }
            self.i += j + 1;
            return if c == '"' {
                Tok::Str(out)
            } else {
                Tok::Ident(out)
            };
        }
        if c.is_ascii_digit() {
            let end = rest
                .find(|x: char| !(x.is_ascii_digit() || x == '.'))
                .unwrap_or(rest.len());
            self.i += end;
            return Tok::Num(rest[..end].parse().unwrap_or(0.0));
        }
        if c.is_alphabetic() || c == '_' || c == '#' {
            let end = rest
                .char_indices()
                .skip(1)
                .find(|(_, x)| !(x.is_alphanumeric() || *x == '_' || *x == '.'))
                .map(|(i, _)| i)
                .unwrap_or(rest.len());
            self.i += end;
            return Tok::Ident(rest[..end].trim_end_matches('.').to_string());
        }
        self.i += c.len_utf8();
        Tok::Sym(c)
    }

    fn peek(&mut self) -> Tok {
        let i = self.i;
        let t = self.next();
        self.i = i;
        t
    }
}

fn expect(l: &mut Lexer, c: char) -> Result<(), String> {
    match l.next() {
        Tok::Sym(x) if x == c => Ok(()),
        t => Err(format!("expected '{c}', found {t:?}")),
    }
}

fn primary(l: &mut Lexer) -> Result<M, String> {
    let mut e = match l.next() {
        Tok::Str(s) => M::Str(s),
        Tok::Num(n) => M::Num(n),
        Tok::Ident(w) if w == "null" => M::Null,
        Tok::Ident(w) if w == "true" || w == "false" => M::Bool(w == "true"),
        Tok::Ident(w) if w == "type" => type_expr(l)?,
        Tok::Ident(w) if w == "let" => {
            let (steps, result) = let_body(l)?;
            M::Let(steps, Box::new(result))
        }
        Tok::Ident(w) if w == "each" => {
            let start = l.i;
            // The function body runs to the end of the argument.
            let mut depth = 0i32;
            loop {
                match l.peek() {
                    Tok::Sym('(' | '[' | '{') => depth += 1,
                    Tok::Sym(')' | ']' | '}') if depth == 0 => break,
                    Tok::Sym(')' | ']' | '}') => depth -= 1,
                    Tok::Sym(',') if depth == 0 => break,
                    Tok::Eof => break,
                    _ => {}
                }
                l.next();
            }
            M::Other(format!("each {}", l.s[start..l.i].trim()))
        }
        Tok::Ident(w) if w.ends_with(".Type") => M::Type(w),
        Tok::Ident(w) => M::Ident(w),
        Tok::Sym('(') => {
            let e = expr(l)?;
            expect(l, ')')?;
            e
        }
        Tok::Sym('[') => {
            let mut fields = Vec::new();
            if l.peek() != Tok::Sym(']') {
                loop {
                    let name = match l.next() {
                        Tok::Ident(n) | Tok::Str(n) => n,
                        t => return Err(format!("unexpected {t:?} in a record")),
                    };
                    expect(l, '=')?;
                    fields.push((name, expr(l)?));
                    match l.next() {
                        Tok::Sym(',') => continue,
                        Tok::Sym(']') => break,
                        t => return Err(format!("unexpected {t:?} in a record")),
                    }
                }
            } else {
                l.next();
            }
            M::Record(fields)
        }
        Tok::Sym('{') => M::List(items(l, '}')?),
        Tok::Sym('-') => match l.next() {
            Tok::Num(n) => M::Num(-n),
            t => return Err(format!("unexpected {t:?}")),
        },
        t => return Err(format!("unexpected {t:?}")),
    };
    // Calls, items and fields.
    loop {
        match l.peek() {
            Tok::Sym('(') => {
                l.next();
                e = M::Call(Box::new(e), items(l, ')')?);
            }
            Tok::Sym('{') => {
                l.next();
                let i = expr(l)?;
                expect(l, '}')?;
                e = M::Item(Box::new(e), Box::new(i));
            }
            Tok::Sym('[') if !matches!(e, M::Str(_) | M::Num(_)) => {
                l.next();
                // A generalized identifier: `[Order Date]` is one name.
                let mut parts = Vec::new();
                loop {
                    match l.next() {
                        Tok::Ident(n) | Tok::Str(n) => parts.push(n),
                        Tok::Num(n) => parts.push(n.to_string()),
                        Tok::Sym(']') if !parts.is_empty() => break,
                        t => return Err(format!("unexpected {t:?} in a field access")),
                    }
                }
                e = M::Field(Box::new(e), parts.join(" "));
            }
            _ => return Ok(e),
        }
    }
}

fn items(l: &mut Lexer, close: char) -> Result<Vec<M>, String> {
    let mut out = Vec::new();
    if l.peek() == Tok::Sym(close) {
        l.next();
        return Ok(out);
    }
    loop {
        out.push(expr(l)?);
        match l.next() {
            Tok::Sym(',') => continue,
            Tok::Sym(c) if c == close => return Ok(out),
            t => return Err(format!("unexpected {t:?} in a list")),
        }
    }
}

/// After `type`: `nullable text`, `table [A = text]`, `number`…
fn type_expr(l: &mut Lexer) -> Result<M, String> {
    match l.next() {
        Tok::Ident(n) if n == "nullable" => type_expr(l),
        Tok::Ident(n) if n == "table" && l.peek() == Tok::Sym('[') => match primary(l)? {
            M::Record(fields) => Ok(M::TableType(fields)),
            _ => Err("unexpected table type".into()),
        },
        Tok::Ident(t) => Ok(M::Type(t)),
        Tok::Sym('(') => {
            let e = expr(l)?;
            expect(l, ')')?;
            Ok(e)
        }
        t => Err(format!("unexpected {t:?} after type")),
    }
}

/// After `let`: the named steps, `in`, and the result.
fn let_body(l: &mut Lexer) -> Result<(Vec<(String, M)>, M), String> {
    let mut steps = Vec::new();
    loop {
        let name = match l.next() {
            Tok::Ident(n) => n,
            t => return Err(format!("unexpected {t:?} where a step name belongs")),
        };
        expect(l, '=')?;
        steps.push((name, expr(l)?));
        match l.next() {
            Tok::Sym(',') => continue,
            Tok::Ident(w) if w == "in" => break,
            t => {
                return Err(format!(
                    "unexpected {t:?} after step {}",
                    steps.last().unwrap().0
                ));
            }
        }
    }
    Ok((steps, expr(l)?))
}

fn product(l: &mut Lexer) -> Result<M, String> {
    let mut e = primary(l)?;
    while let Tok::Sym(c @ ('*' | '/')) = l.peek() {
        l.next();
        e = M::Bin(Box::new(e), c, Box::new(primary(l)?));
    }
    Ok(e)
}

fn expr(l: &mut Lexer) -> Result<M, String> {
    let mut e = product(l)?;
    loop {
        match l.peek() {
            Tok::Sym('&') => {
                l.next();
                e = M::Concat(Box::new(e), Box::new(product(l)?));
            }
            Tok::Sym(c @ ('+' | '-')) => {
                l.next();
                e = M::Bin(Box::new(e), c, Box::new(product(l)?));
            }
            _ => break,
        }
    }
    // ` meta [...]` on parameters: the value is what matters.
    if l.peek() == Tok::Ident("meta".into()) {
        l.next();
        primary(l)?;
    }
    Ok(e)
}

/// A `let … in …` document as its named steps and the name it returns.
fn document(text: &str) -> Result<(Vec<(String, M)>, String), String> {
    let mut l = Lexer { s: text, i: 0 };
    if l.peek() != Tok::Ident("let".into()) {
        let e = expr(&mut l)?;
        return Ok((vec![("Source".into(), e)], "Source".into()));
    }
    l.next();
    match let_body(&mut l)? {
        (steps, M::Ident(result)) => Ok((steps, result)),
        _ => Err("its result isn't one of its steps".into()),
    }
}

/// A text value: a literal, or a literal with `meta` (a parameter's definition).
pub fn string_literal(text: &str) -> Option<String> {
    let mut l = Lexer { s: text, i: 0 };
    match expr(&mut l).ok()? {
        M::Str(s) => Some(s),
        _ => None,
    }
}

fn text(m: &M, params: &BTreeMap<String, String>) -> Option<String> {
    match m {
        M::Str(s) => Some(s.clone()),
        M::Ident(p) => params.get(p).cloned(),
        M::Concat(a, b) => Some(text(a, params)? + &text(b, params)?),
        _ => None,
    }
}

/// `File.Contents(path)` → the path.
fn file(m: &M, params: &BTreeMap<String, String>) -> Result<String, String> {
    match m {
        M::Call(f, args) if matches!(&**f, M::Ident(n) if n == "File.Contents") => {
            let p = args
                .first()
                .and_then(|a| text(a, params))
                .ok_or("the file path isn't plain text")?;
            // Windows paths: forward slashes, so they read on Linux once the folder is right.
            Ok(p.replace('\\', "/"))
        }
        // A file on the web: its address, with a relative path when one is given.
        M::Call(f, args) if matches!(&**f, M::Ident(n) if n == "Web.Contents") => {
            let mut url = args
                .first()
                .and_then(|a| text(a, params))
                .ok_or("the web address isn't plain text")?;
            if let Some(M::Record(fs)) = args.get(1) {
                for (k, v) in fs {
                    match (k.as_str(), text(v, params)) {
                        ("RelativePath", Some(rel)) if !rel.is_empty() => {
                            url = format!(
                                "{}/{}",
                                url.trim_end_matches('/'),
                                rel.trim_start_matches('/')
                            );
                        }
                        ("RelativePath", _) => {}
                        _ => {
                            return Err(format!(
                                "reads the web with {k}, which charted doesn't send"
                            ));
                        }
                    }
                }
            }
            if !charted_compiler::steps::is_url(&url) {
                return Err("the web address isn't http or https".into());
            }
            Ok(url)
        }
        M::Call(f, _) => Err(format!("reads data with {}", name_of(f))),
        _ => Err("the source isn't a file".into()),
    }
}

fn name_of(m: &M) -> String {
    match m {
        M::Ident(n) => n.clone(),
        _ => "an expression".into(),
    }
}

fn texts(m: &M) -> Option<Vec<String>> {
    match m {
        M::List(xs) => xs
            .iter()
            .map(|x| {
                if let M::Str(s) = x {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .collect(),
        M::Str(s) => Some(vec![s.clone()]),
        _ => None,
    }
}

fn m_type(t: &M) -> Option<DataType> {
    let M::Type(t) = t else { return None };
    Some(match t.to_ascii_lowercase().as_str() {
        "text" | "text.type" => DataType::String,
        "int64.type" | "int32.type" | "int16.type" | "int8.type" => DataType::Int64,
        "number" | "number.type" | "double.type" | "single.type" | "percentage.type" => {
            DataType::Double
        }
        "currency.type" | "decimal.type" => DataType::Decimal,
        "logical" | "logical.type" => DataType::Boolean,
        "date" | "date.type" => DataType::Date,
        "datetime" | "datetime.type" => DataType::DateTime,
        "datetimezone" | "datetimezone.type" => DataType::DateTimeTz,
        "time" | "time.type" => DataType::Time,
        "binary" | "binary.type" => DataType::Binary,
        _ => return None,
    })
}

/// A value worked out while importing: what a calendar's bounds are made of.
#[derive(Debug, Clone, PartialEq)]
enum V {
    Num(f64),
    /// Days since 1970-01-01.
    Date(i64),
    Text(String),
    Bool(bool),
}

/// Days since 1970-01-01 of a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (
        if m <= 2 {
            yoe + era * 400 + 1
        } else {
            yoe + era * 400
        },
        m,
        d,
    )
}

fn iso_date(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

fn today() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| (d.as_secs() / 86400) as i64)
        .unwrap_or(0)
}

/// Evaluates the constant parts of M a calendar's bounds use: dates, numbers, the
/// current date, and earlier steps holding such values.
fn eval(m: &M, env: &BTreeMap<String, V>) -> Option<V> {
    let num = |x: &M| match eval(x, env)? {
        V::Num(n) => Some(n),
        _ => None,
    };
    let date = |x: &M| match eval(x, env)? {
        V::Date(d) => Some(d),
        _ => None,
    };
    Some(match m {
        M::Num(n) => V::Num(*n),
        M::Str(s) => V::Text(s.clone()),
        M::Bool(b) => V::Bool(*b),
        M::Ident(n) => env.get(n)?.clone(),
        M::Bin(a, op, b) => match (eval(a, env)?, *op, eval(b, env)?) {
            (V::Num(a), '+', V::Num(b)) => V::Num(a + b),
            (V::Num(a), '-', V::Num(b)) => V::Num(a - b),
            (V::Num(a), '*', V::Num(b)) => V::Num(a * b),
            (V::Num(a), '/', V::Num(b)) if b != 0.0 => V::Num(a / b),
            // Durations are numbers of days.
            (V::Date(a), '-', V::Date(b)) => V::Num((a - b) as f64),
            (V::Date(a), '+', V::Num(b)) if b.fract() == 0.0 => V::Date(a + b as i64),
            (V::Date(a), '-', V::Num(b)) if b.fract() == 0.0 => V::Date(a - b as i64),
            _ => return None,
        },
        M::Call(f, args) => {
            let M::Ident(f) = &**f else { return None };
            let arg = |i: usize| args.get(i);
            match (f.as_str(), args.len()) {
                ("#date", 3) => {
                    let (y, mo, d) = (num(arg(0)?)?, num(arg(1)?)?, num(arg(2)?)?);
                    if !(1.0..=12.0).contains(&mo) || !(1.0..=31.0).contains(&d) {
                        return None;
                    }
                    V::Date(days_from_civil(y as i64, mo as i64, d as i64))
                }
                ("#duration", 4) => {
                    let parts: Vec<f64> = args.iter().map(num).collect::<Option<_>>()?;
                    if parts[1..].iter().any(|p| *p != 0.0) {
                        return None;
                    }
                    V::Num(parts[0])
                }
                (
                    "DateTime.LocalNow"
                    | "DateTime.FixedLocalNow"
                    | "DateTimeZone.LocalNow"
                    | "DateTimeZone.FixedLocalNow"
                    | "DateTimeZone.UtcNow"
                    | "DateTimeZone.FixedUtcNow",
                    0,
                ) => V::Date(today()),
                ("Date.From" | "DateTime.Date" | "Date.FromText", 1) => match eval(arg(0)?, env)? {
                    V::Text(t) => {
                        let p: Vec<i64> = t
                            .split('-')
                            .map(|x| x.parse().ok())
                            .collect::<Option<_>>()?;
                        match p[..] {
                            [y, mo, d] => V::Date(days_from_civil(y, mo, d)),
                            _ => return None,
                        }
                    }
                    v @ V::Date(_) => v,
                    _ => return None,
                },
                ("Duration.Days" | "Duration.From" | "Number.From", 1) => V::Num(num(arg(0)?)?),
                ("Date.Year", 1) => V::Num(civil_from_days(date(arg(0)?)?).0 as f64),
                ("Date.Month", 1) => V::Num(civil_from_days(date(arg(0)?)?).1 as f64),
                ("Date.Day", 1) => V::Num(civil_from_days(date(arg(0)?)?).2 as f64),
                ("Date.StartOfYear", 1) => {
                    V::Date(days_from_civil(civil_from_days(date(arg(0)?)?).0, 1, 1))
                }
                ("Date.EndOfYear", 1) => {
                    V::Date(days_from_civil(civil_from_days(date(arg(0)?)?).0, 12, 31))
                }
                ("Date.AddDays", 2) => V::Date(date(arg(0)?)? + num(arg(1)?)? as i64),
                ("Date.AddYears", 2) => {
                    let (y, mo, d) = civil_from_days(date(arg(0)?)?);
                    V::Date(days_from_civil(y + num(arg(1)?)? as i64, mo, d.min(28)))
                }
                _ => return None,
            }
        }
        _ => return None,
    })
}

/// A cell of a `#table` or literal row list, as charted keeps entered values (text).
fn cell(m: &M, env: &BTreeMap<String, V>) -> Result<Option<String>, String> {
    Ok(match m {
        M::Null => None,
        M::Str(s) => Some(s.clone()),
        M::Bool(b) => Some(b.to_string()),
        _ => match eval(m, env) {
            Some(V::Num(n)) if n.fract() == 0.0 && n.abs() < 1e15 => Some(format!("{}", n as i64)),
            Some(V::Num(n)) => Some(n.to_string()),
            // Only a written date: "today" would be frozen at import.
            Some(V::Date(d)) if !matches!(m, M::Ident(_)) && !uses_now(m) => Some(iso_date(d)),
            _ => return Err("a value in its table isn't a constant".into()),
        },
    })
}

fn uses_now(m: &M) -> bool {
    match m {
        M::Call(f, args) => {
            matches!(&**f, M::Ident(n) if n.ends_with("Now")) || args.iter().any(uses_now)
        }
        M::Bin(a, _, b) | M::Concat(a, b) => uses_now(a) || uses_now(b),
        _ => false,
    }
}

/// The rows of a literal list of lists.
fn literal_rows(m: &M, env: &BTreeMap<String, V>) -> Result<Vec<Vec<Option<String>>>, String> {
    let M::List(rows) = m else {
        return Err("its rows aren't a list".into());
    };
    rows.iter()
        .map(|r| match r {
            M::List(cells) => cells.iter().map(|c| cell(c, env)).collect(),
            _ => Err("its rows aren't lists".into()),
        })
        .collect()
}

/// The type of a column in `type table [A = text]` (or `#table`'s list of names).
fn column_type(m: &M) -> Option<DataType> {
    match m {
        M::Ident(t) => m_type(&M::Type(t.clone())),
        t => m_type(t),
    }
}

/// The type the values of an untyped column read as: numbers, dates, true/false or text.
fn literal_type(rows: &[Vec<Option<String>>], i: usize, m_rows: &M) -> DataType {
    let M::List(rows_m) = m_rows else {
        return DataType::String;
    };
    let cells: Vec<&M> = rows_m
        .iter()
        .filter_map(|r| match r {
            M::List(c) => c.get(i).filter(|c| **c != M::Null),
            _ => None,
        })
        .collect();
    if cells.is_empty() {
        return DataType::String;
    }
    if cells.iter().all(|c| matches!(c, M::Num(_) | M::Bin(..))) {
        let whole = rows
            .iter()
            .filter_map(|r| r.get(i).cloned().flatten())
            .all(|v| !v.contains('.'));
        return if whole {
            DataType::Int64
        } else {
            DataType::Double
        };
    }
    if cells.iter().all(|c| matches!(c, M::Bool(_))) {
        return DataType::Boolean;
    }
    if cells
        .iter()
        .all(|c| matches!(c, M::Call(f, _) if matches!(&**f, M::Ident(n) if n == "#date")))
    {
        return DataType::Date;
    }
    DataType::String
}

/// Cultures that write numbers "1.234,5" (their decimal separator is a comma).
pub fn decimal_comma_culture(culture: &str) -> bool {
    let c = culture.to_ascii_lowercase().replace('_', "-");
    let lang = c.split('-').next().unwrap_or("");
    const COMMA: &[&str] = &[
        "af", "az", "be", "bg", "bs", "ca", "cs", "da", "de", "el", "es", "et", "eu", "fi", "fo",
        "fr", "gl", "hr", "hu", "hy", "id", "is", "it", "ka", "kk", "ky", "lt", "lv", "mk", "mn",
        "nb", "nl", "nn", "no", "pl", "pt", "ro", "ru", "sk", "sl", "sq", "sr", "sv", "tr", "uk",
        "uz", "vi",
    ];
    // Regions that write a decimal point although their language usually doesn't.
    const POINT: &[&str] = &[
        "de-ch", "de-li", "fr-ch", "it-ch", "es-mx", "es-us", "es-pr", "es-do", "es-gt", "es-hn",
        "es-ni", "es-pa", "es-pe", "es-sv",
    ];
    COMMA.contains(&lang) && !POINT.contains(&c.as_str())
}

fn count(m: &M) -> Option<u32> {
    match m {
        M::Num(n) if *n >= 0.0 && n.fract() == 0.0 => Some(*n as u32),
        _ => None,
    }
}

/// A source and steps for an M query that loads a file, or why it can't be.
///
/// A query that starts from another one (`Source = #"Other"`) gives a
/// `Source::Reference` holding the other query's *name*: the caller resolves it.
pub fn to_query(
    text_m: &str,
    params: &BTreeMap<String, String>,
) -> Result<(Source, Vec<Step>), String> {
    let (source, mut steps) = to_query_at(text_m, params)?;
    // charted's own calendar comes back as just its source.
    if let Source::Calendar { start, end, .. } = &source {
        for fiscal_start in 1..=12 {
            let own = Source::Calendar {
                start: start.clone(),
                end: end.clone(),
                fiscal_start,
            };
            let q = charted_compiler::steps::Query {
                id: String::new(),
                name: String::new(),
                table: String::new(),
                source: own.clone(),
                steps: vec![],
                step_names: Default::default(),
                no_load: false,
            };
            let text = charted_compiler::steps::to_m(&q, &[]);
            if let Ok((_, made)) = to_query_at(&text, params)
                && steps.starts_with(&made)
            {
                steps.drain(..made.len());
                return Ok((own, steps));
            }
        }
    }
    Ok((source, steps))
}

fn to_query_at(
    text_m: &str,
    params: &BTreeMap<String, String>,
) -> Result<(Source, Vec<Step>), String> {
    let (steps, result) = document(text_m)?;
    let end = steps
        .iter()
        .position(|(n, _)| *n == result)
        .ok_or("its result isn't one of its steps")?;
    let mut source: Option<Source> = None;
    let mut out: Vec<Step> = Vec::new();
    let mut prev: Option<&str> = None;
    // Whether the previous step only named a workbook (its sheet comes next).
    let mut workbook: Option<String> = None;
    // Steps holding values (a calendar's bounds), by name.
    let mut env: BTreeMap<String, V> = BTreeMap::new();
    // A calendar's list of dates, not yet a table.
    let mut dates = false;
    // An entered table's columns are text until its first type change.
    let mut untyped = false;
    for (name, e) in &steps[..=end] {
        let call = match e {
            M::Call(f, args) => Some((name_of(f), args.as_slice())),
            _ => None,
        };
        // The first argument of a table function must be the previous step.
        let on_prev =
            |args: &[M]| matches!(args.first(), Some(M::Ident(p)) if Some(p.as_str()) == prev);
        if dates && !matches!(&call, Some((f, args)) if f == "Table.FromList" && on_prev(args)) {
            return Err(format!(
                "step \"{name}\" uses its list of dates as something other than a table"
            ));
        }
        match (&source, call, e) {
            (None, Some((f, args)), _) if f == "List.Dates" => {
                let (Some(V::Date(start)), Some(V::Num(n)), Some(V::Num(by))) = (
                    args.first().and_then(|a| eval(a, &env)),
                    args.get(1).and_then(|a| eval(a, &env)),
                    args.get(2).and_then(|a| eval(a, &env)),
                ) else {
                    return Err(
                        "its list of dates doesn't start and end on dates charted can work out"
                            .into(),
                    );
                };
                if by != 1.0 || n < 1.0 || n.fract() != 0.0 {
                    return Err("its list of dates isn't one row per day".into());
                }
                source = Some(Source::Calendar {
                    start: iso_date(start),
                    end: iso_date(start + n as i64 - 1),
                    fiscal_start: 1,
                });
                dates = true;
            }
            (None, Some((f, args)), _) if f == "Table.FromRows" || f == "#table" => {
                let (names, rows_m) = if f == "#table" {
                    (args.first(), args.get(1))
                } else {
                    (args.get(1), args.first())
                };
                let rows = match rows_m {
                    // Enter data: the rows as compressed JSON.
                    Some(M::Call(j, a)) if name_of(j) == "Json.Document" => {
                        let text = match a.first() {
                            Some(M::Call(d, a)) if name_of(d) == "Binary.Decompress" => {
                                match a.first() {
                                    Some(M::Call(b, a)) if name_of(b) == "Binary.FromText" => {
                                        match a.first() {
                                            Some(M::Str(s)) => Some(s.clone()),
                                            _ => None,
                                        }
                                    }
                                    _ => None,
                                }
                            }
                            _ => None,
                        };
                        charted_compiler::steps::table_rows_from_m(
                            &text.ok_or("its table's data isn't written in the query")?,
                        )?
                    }
                    Some(m) => literal_rows(m, &env)?,
                    None => return Err(format!("{f} needs rows")),
                };
                let names = match names {
                    Some(M::Let(_, t)) => &**t,
                    Some(t) => t,
                    None => return Err(format!("{f} needs columns")),
                };
                let mut columns: Vec<(String, DataType)> = match names {
                    M::TableType(fields) => fields
                        .iter()
                        .map(|(n, t)| (n.clone(), column_type(t).unwrap_or(DataType::String)))
                        .collect(),
                    M::Num(n) if n.fract() == 0.0 && *n >= 1.0 => (1..=*n as usize)
                        .map(|i| (format!("Column{i}"), DataType::String))
                        .collect(),
                    m => texts(m)
                        .ok_or("its columns aren't named")?
                        .into_iter()
                        .map(|n| (n, DataType::String))
                        .collect(),
                };
                // A #table of numbers or dates has them as such.
                if let (M::List(_), Some(r)) = (names, rows_m) {
                    for (i, c) in columns.iter_mut().enumerate() {
                        c.1 = literal_type(&rows, i, r);
                    }
                }
                let rows = rows
                    .into_iter()
                    .map(|mut r| {
                        r.resize(columns.len(), None);
                        r
                    })
                    .collect();
                untyped = matches!(names, M::TableType(_) | M::Num(_))
                    && columns.iter().all(|c| c.1 == DataType::String);
                source = Some(Source::Table { columns, rows });
            }
            (None, None, M::Ident(other))
                if !params.contains_key(other) && !env.contains_key(other) =>
            {
                source = Some(Source::Reference {
                    query: other.clone(),
                });
            }
            (None, _, _) if eval(e, &env).is_some() => {
                env.insert(name.clone(), eval(e, &env).unwrap_or(V::Num(0.0)));
                continue;
            }
            (None, Some((f, args)), _) if f == "Csv.Document" => {
                let path = file(args.first().ok_or("Csv.Document needs a file")?, params)?;
                let option = |key: &str| match args.get(1) {
                    Some(M::Record(fs)) => fs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
                    _ => None,
                };
                let delimiter = match option("Delimiter") {
                    Some(M::Str(s)) => Some(s.clone()),
                    _ => None,
                };
                let encoding = match option("Encoding") {
                    Some(M::Num(n)) => {
                        charted_compiler::steps::encoding_of_code_page(*n as u32).map(String::from)
                    }
                    _ => None,
                };
                source = Some(Source::Csv {
                    path,
                    delimiter: delimiter.filter(|d| d != ","),
                    header: false,
                    encoding,
                    decimal: None,
                });
            }
            (None, Some((f, args)), _) if f == "Excel.Workbook" => {
                let path = file(args.first().ok_or("Excel.Workbook needs a file")?, params)?;
                if charted_compiler::steps::is_url(&path) {
                    return Err("reads an Excel workbook from the web".into());
                }
                workbook = Some(path);
            }
            (None, Some((f, args)), _) if f == "Parquet.Document" => {
                source = Some(Source::Parquet {
                    path: file(args.first().ok_or("Parquet.Document needs a file")?, params)?,
                });
            }
            (None, Some((f, args)), _) if f == "Json.Document" => {
                source = Some(Source::Json {
                    path: file(args.first().ok_or("Json.Document needs a file")?, params)?,
                });
            }
            (None, None, M::Field(inner, field)) if workbook.is_some() && field == "Data" => {
                // Source{[Item="Sheet1",Kind="Sheet"]}[Data] or Source{[Name="Sheet1"]}[Data]
                let M::Item(_, key) = &**inner else {
                    return Err("the workbook's sheet isn't named".into());
                };
                let M::Record(fs) = &**key else {
                    return Err("the workbook's sheet isn't named".into());
                };
                let sheet = fs
                    .iter()
                    .find(|(k, _)| k == "Item" || k == "Name")
                    .and_then(|(_, v)| {
                        if let M::Str(s) = v {
                            Some(s.clone())
                        } else {
                            None
                        }
                    })
                    .ok_or("the workbook's sheet isn't named")?;
                if fs
                    .iter()
                    .any(|(k, v)| k == "Kind" && *v != M::Str("Sheet".into()))
                {
                    return Err("it reads a table or named range of a workbook, not a sheet".into());
                }
                source = Some(Source::Excel {
                    path: workbook.take().unwrap_or_default(),
                    sheet,
                    header: false,
                });
            }
            (Some(_), Some((f, args)), _) if on_prev(args) => match f.as_str() {
                "Table.PromoteHeaders" if out.is_empty() => match source.as_mut() {
                    Some(Source::Csv { header, .. } | Source::Excel { header, .. }) if !*header => {
                        *header = true
                    }
                    _ => return Err("it promotes headers after other steps".into()),
                },
                "Table.FromRecords" | "Table.FromList"
                    if out.is_empty() && matches!(source, Some(Source::Json { .. })) => {}
                "Table.FromList" if dates => {
                    // The calendar's own columns go; the query adds those it wants.
                    let column = match args.get(2) {
                        None | Some(M::Null) => "Column1".to_string(),
                        Some(M::TableType(fields)) if fields.len() == 1 => fields[0].0.clone(),
                        Some(m) => match texts(m).as_deref() {
                            Some([one]) => one.clone(),
                            _ => return Err("its dates become more than one column".into()),
                        },
                    };
                    out.push(Step::SelectColumns {
                        columns: vec!["Date".into()],
                    });
                    if column != "Date" {
                        out.push(Step::RenameColumns {
                            renames: vec![("Date".into(), column)],
                        });
                    }
                    dates = false;
                }
                "Table.AddColumn" => {
                    let (Some(M::Str(column)), Some(M::Other(f))) = (args.get(1), args.get(2))
                    else {
                        return Err(format!("step \"{name}\" adds a column charted can't read"));
                    };
                    let formula = f.strip_prefix("each").unwrap_or(f).trim().to_string();
                    charted_compiler::mformula::parse(&formula, &[]).map_err(|e| {
                        format!("step \"{name}\" adds a column with a formula charted doesn't know ({e})")
                    })?;
                    out.push(Step::CustomColumn {
                        name: column.clone(),
                        formula,
                    });
                }
                "Table.TransformColumnTypes" => {
                    let M::List(pairs) = args
                        .get(1)
                        .ok_or("Table.TransformColumnTypes needs types")?
                    else {
                        return Err("its type changes aren't a list".into());
                    };
                    let mut columns = Vec::new();
                    for p in pairs {
                        let M::List(kv) = p else {
                            return Err("its type changes aren't pairs".into());
                        };
                        match (kv.first(), kv.get(1).and_then(m_type)) {
                            (Some(M::Str(c)), Some(t)) => columns.push((c.clone(), t)),
                            _ => {
                                return Err(
                                    "it changes a column to a type charted doesn't know".into()
                                );
                            }
                        }
                    }
                    // The culture numbers are read in: "1.234,5" in German.
                    let culture = match args.get(2) {
                        Some(M::Str(c)) => Some(c.as_str()),
                        Some(M::Record(fs)) => fs.iter().find_map(|(k, v)| match v {
                            M::Str(c) if k == "Culture" => Some(c.as_str()),
                            _ => None,
                        }),
                        _ => None,
                    };
                    if let (Some(c), Some(Source::Csv { decimal, .. })) = (culture, source.as_mut())
                        && decimal_comma_culture(c)
                    {
                        *decimal = Some(",".into());
                    }
                    // An entered table's first type change sets its columns' types.
                    if let Some(Source::Table { columns: cols, .. }) = source.as_mut()
                        && untyped
                        && out.is_empty()
                        && columns.iter().all(|(c, _)| cols.iter().any(|x| x.0 == *c))
                    {
                        for (c, t) in columns {
                            if let Some(x) = cols.iter_mut().find(|x| x.0 == c) {
                                x.1 = t;
                            }
                        }
                        untyped = false;
                    } else {
                        out.push(Step::ChangeTypes { columns });
                    }
                }
                "Table.RemoveColumns" => out.push(Step::RemoveColumns {
                    columns: args
                        .get(1)
                        .and_then(texts)
                        .ok_or("its columns aren't a list")?,
                }),
                "Table.SelectColumns" => out.push(Step::SelectColumns {
                    columns: args
                        .get(1)
                        .and_then(texts)
                        .ok_or("its columns aren't a list")?,
                }),
                "Table.RenameColumns" => {
                    let M::List(pairs) = args.get(1).ok_or("Table.RenameColumns needs names")?
                    else {
                        return Err("its renames aren't a list".into());
                    };
                    let renames = pairs
                        .iter()
                        .map(|p| match p {
                            M::List(kv) => match (kv.first(), kv.get(1)) {
                                (Some(M::Str(a)), Some(M::Str(b))) => Some((a.clone(), b.clone())),
                                _ => None,
                            },
                            _ => None,
                        })
                        .collect::<Option<Vec<_>>>()
                        .ok_or("its renames aren't pairs of names")?;
                    out.push(Step::RenameColumns { renames });
                }
                "Table.Skip" => out.push(Step::RemoveTopRows {
                    count: args
                        .get(1)
                        .and_then(count)
                        .ok_or("it skips a number of rows charted can't read")?,
                }),
                "Table.FirstN" => out.push(Step::KeepTopRows {
                    count: args
                        .get(1)
                        .and_then(count)
                        .ok_or("it keeps a number of rows charted can't read")?,
                }),
                "Table.Distinct" if args.len() == 1 => out.push(Step::RemoveDuplicates {
                    columns: Vec::new(),
                }),
                other => return Err(format!("step \"{name}\" uses {other}")),
            },
            (None, Some((f, _)), _) => return Err(format!("it reads data with {f}")),
            _ => return Err(format!("step \"{name}\" isn't one charted understands")),
        }
        prev = Some(name);
    }
    match source {
        _ if dates => Err("it makes a list of dates, not a table".into()),
        Some(s) => Ok((s, out)),
        None if workbook.is_some() => Err("it reads a whole workbook, not one sheet".into()),
        None => Err("it doesn't read a file".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_with_headers_and_types() {
        let params = BTreeMap::from([("SourceFolder".to_string(), "/data/".to_string())]);
        let m = r#"let
    Source = Csv.Document(File.Contents(SourceFolder & "sales.csv"), [Delimiter=";", Columns=3, Encoding=65001, QuoteStyle=QuoteStyle.Csv]),
    #"Promoted Headers" = Table.PromoteHeaders(Source, [PromoteAllScalars=true]),
    #"Changed Type" = Table.TransformColumnTypes(#"Promoted Headers",{{"Date", type date}, {"Amount", Currency.Type}, {"Units", Int64.Type}}),
    #"Removed Columns" = Table.RemoveColumns(#"Changed Type",{"Notes"}),
    #"Renamed" = Table.RenameColumns(#"Removed Columns",{{"Units", "Quantity"}})
in
    #"Renamed""#;
        let (s, steps) = to_query(m, &params).unwrap();
        assert_eq!(
            s,
            Source::Csv {
                path: "/data/sales.csv".into(),
                delimiter: Some(";".into()),
                header: true,
                encoding: None,
                decimal: None
            }
        );
        assert_eq!(steps.len(), 3);
        assert_eq!(
            steps[0],
            Step::ChangeTypes {
                columns: vec![
                    ("Date".into(), DataType::Date),
                    ("Amount".into(), DataType::Decimal),
                    ("Units".into(), DataType::Int64)
                ]
            }
        );
    }

    #[test]
    fn references_name_the_other_query() {
        let m = "let\n    Source = #\"RAW-Customer\",\n    #\"Renamed Columns\" = Table.RenameColumns(Source,{{\"Name\", \"Customer\"}})\nin\n    #\"Renamed Columns\"";
        let (s, steps) = to_query(m, &BTreeMap::new()).unwrap();
        assert_eq!(
            s,
            Source::Reference {
                query: "RAW-Customer".into()
            }
        );
        assert_eq!(steps.len(), 1);
        // A parameter isn't a query.
        let params = BTreeMap::from([("Folder".to_string(), "/x".to_string())]);
        assert!(to_query("let Source = Folder in Source", &params).is_err());
    }

    #[test]
    fn entered_tables_from_power_bi_and_table_literals() {
        // As Power BI's Enter data writes it.
        let m = r#"let
    Source = Table.FromRows(Json.Document(Binary.Decompress(Binary.FromText("i45WSlTSUTJUitWJVkoCsoyUYmMB", BinaryEncoding.Base64), Compression.Deflate)), let _t = ((type nullable text) meta [Serialized.Text = true]) in type table [Key = _t, #"Sort Order" = _t]),
    #"Changed Type" = Table.TransformColumnTypes(Source,{{"Key", type text}, {"Sort Order", Int64.Type}})
in
    #"Changed Type""#;
        let (s, steps) = to_query(m, &BTreeMap::new()).unwrap();
        assert_eq!(
            s,
            Source::Table {
                columns: vec![
                    ("Key".into(), DataType::String),
                    ("Sort Order".into(), DataType::Int64)
                ],
                rows: vec![
                    vec![Some("a".into()), Some("1".into())],
                    vec![Some("b".into()), Some("2".into())]
                ]
            }
        );
        assert!(steps.is_empty());
        // charted's own export of the same comes back the same.
        let q = charted_compiler::steps::Query {
            id: "q".into(),
            name: "T".into(),
            table: "t".into(),
            source: s.clone(),
            steps: vec![Step::RemoveTopRows { count: 1 }],
            step_names: Default::default(),
            no_load: false,
        };
        let back = to_query(&charted_compiler::steps::to_m(&q, &[]), &BTreeMap::new()).unwrap();
        assert_eq!(back, (s, q.steps.clone()));
        // #table with names and values.
        let m = r#"let Source = #table({"Key", "Value", "When"}, {{"a", 1.5, #date(2024, 2, 29)}, {"b", null, #date(2024, 3, 1)}}) in Source"#;
        let (s, _) = to_query(m, &BTreeMap::new()).unwrap();
        assert_eq!(
            s,
            Source::Table {
                columns: vec![
                    ("Key".into(), DataType::String),
                    ("Value".into(), DataType::Double),
                    ("When".into(), DataType::Date)
                ],
                rows: vec![
                    vec![
                        Some("a".into()),
                        Some("1.5".into()),
                        Some("2024-02-29".into())
                    ],
                    vec![Some("b".into()), None, Some("2024-03-01".into())]
                ]
            }
        );
        let typed =
            r#"let Source = #table(type table [N = Int64.Type, T = text], {{1, "x"}}) in Source"#;
        let (s, _) = to_query(typed, &BTreeMap::new()).unwrap();
        assert!(
            matches!(&s, Source::Table { columns, .. } if columns[0].1 == DataType::Int64 && columns[1].1 == DataType::String),
            "{s:?}"
        );
        // A value worked out when the query runs isn't kept.
        let now = r#"let Source = #table({"K", "V"}, {{"Last Refresh", DateTime.ToText(DateTime.LocalNow(), "yyyy")}}) in Source"#;
        assert!(
            to_query(now, &BTreeMap::new())
                .unwrap_err()
                .contains("isn't a constant")
        );
    }

    #[test]
    fn calendars_from_lists_of_dates() {
        // charted's own, with a fiscal year from July, comes back as its source.
        let own = Source::Calendar {
            start: "2023-01-01".into(),
            end: "2024-12-31".into(),
            fiscal_start: 7,
        };
        let q = charted_compiler::steps::Query {
            id: "q".into(),
            name: "Calendar".into(),
            table: "calendar".into(),
            source: own.clone(),
            steps: vec![],
            step_names: Default::default(),
            no_load: false,
        };
        let m = charted_compiler::steps::to_m(&q, &[]);
        assert_eq!(to_query(&m, &BTreeMap::new()).unwrap(), (own, vec![]));
        // A common hand-written one: its columns become custom columns.
        let m = r#"let
    StartDate = #date(2020, 1, 1),
    EndDate = #date(2020, 12, 31),
    // Generate a list of dates
    DateList = List.Dates(StartDate, Duration.Days(EndDate - StartDate) + 1, #duration(1, 0, 0, 0)),
    Calendar = Table.FromList(DateList, Splitter.SplitByNothing(), {"Date"}),
    AddYear = Table.AddColumn(Calendar, "Year", each Date.Year([Date])),
    AddMonthName = Table.AddColumn(AddYear, "Month Name", each Date.ToText([Date], "MMM")),
    AddFiscalQuarter = Table.AddColumn(
        AddMonthName, "Fiscal Quarter", each "FQ" & Text.From(Number.IntegerDivide((Date.Month([Date]) + 5), 3))
    ),
    #"Changed Type" = Table.TransformColumnTypes(AddFiscalQuarter, {{"Date", type date}, {"Year", Int64.Type}})
in
    #"Changed Type""#;
        let (s, steps) = to_query(m, &BTreeMap::new()).unwrap();
        assert_eq!(
            s,
            Source::Calendar {
                start: "2020-01-01".into(),
                end: "2020-12-31".into(),
                fiscal_start: 1
            }
        );
        assert_eq!(
            steps[..3],
            [
                Step::SelectColumns {
                    columns: vec!["Date".into()]
                },
                Step::CustomColumn {
                    name: "Year".into(),
                    formula: "Date.Year([Date])".into()
                },
                Step::CustomColumn {
                    name: "Month Name".into(),
                    formula: "Date.ToText([Date], \"MMM\")".into()
                },
            ]
        );
        assert_eq!(steps.len(), 5);
        // Relative to today: the bounds are worked out when importing.
        let m = "let P_Today = DateTime.LocalNow(), S = #date(Date.Year(P_Today) - 3, 1, 1), E = #date(Date.Year(P_Today), 12, 31), L = List.Dates(S, Duration.Days(Duration.From(E - S)) + 1, #duration(1,0,0,0)), T = Table.FromList(L, Splitter.SplitByNothing()) in T";
        let (s, steps) = to_query(m, &BTreeMap::new()).unwrap();
        let year = civil_from_days(today()).0;
        assert_eq!(
            s,
            Source::Calendar {
                start: format!("{}-01-01", year - 3),
                end: format!("{year}-12-31"),
                fiscal_start: 1
            }
        );
        assert_eq!(
            steps[1],
            Step::RenameColumns {
                renames: vec![("Date".into(), "Column1".into())]
            }
        );
        // Bounds read from another query's data can't be worked out.
        let m = "let S = List.Min(#\"Sales\"[Date]), L = List.Dates(S, 10, #duration(1,0,0,0)), T = Table.FromList(L, Splitter.SplitByNothing()) in T";
        assert!(to_query(m, &BTreeMap::new()).is_err());
        assert_eq!(iso_date(days_from_civil(2024, 2, 29)), "2024-02-29");
        assert_eq!(days_from_civil(1970, 1, 1), 0);
    }

    #[test]
    fn csv_culture_sets_the_decimal_comma() {
        let m = |culture: &str| {
            format!(
                "let Source = Csv.Document(File.Contents(\"/d/a.csv\"), [Delimiter=\";\"]), #\"Changed Type\" = Table.TransformColumnTypes(Source, {{{{\"Amount\", type number}}}}{culture}) in #\"Changed Type\""
            )
        };
        let decimal = |culture: &str| match to_query(&m(culture), &BTreeMap::new()).unwrap().0 {
            Source::Csv { decimal, .. } => decimal,
            s => panic!("{s:?}"),
        };
        assert_eq!(decimal(", \"de-DE\"").as_deref(), Some(","));
        assert_eq!(decimal(", [Culture = \"da-DK\"]").as_deref(), Some(","));
        assert_eq!(decimal(", \"en-US\""), None);
        assert_eq!(decimal(", \"de-CH\""), None);
        assert_eq!(decimal(""), None);
    }

    #[test]
    fn excel_sheets_and_what_is_refused() {
        let m = "let\n Source = Excel.Workbook(File.Contents(\"C:\\data\\t.xlsx\"), null, true),\n Sheet1_Sheet = Source{[Item=\"Targets\",Kind=\"Sheet\"]}[Data],\n #\"Promoted Headers\" = Table.PromoteHeaders(Sheet1_Sheet, [PromoteAllScalars=true])\nin\n #\"Promoted Headers\"";
        let (s, _) = to_query(m, &BTreeMap::new()).unwrap();
        assert_eq!(
            s,
            Source::Excel {
                path: "C:/data/t.xlsx".into(),
                sheet: "Targets".into(),
                header: true
            }
        );
        let sql = "let Source = Sql.Database(\"srv\", \"db\") in Source";
        assert_eq!(
            to_query(sql, &BTreeMap::new()).unwrap_err(),
            "it reads data with Sql.Database"
        );
        let filter = "let Source = Parquet.Document(File.Contents(\"/x.parquet\")), F = Table.SelectRows(Source, each [A] > 1) in F";
        assert!(
            to_query(filter, &BTreeMap::new())
                .unwrap_err()
                .contains("Table.SelectRows")
        );
        assert_eq!(
            string_literal("\"C:\\\\data\\\\\" meta [IsParameterQuery=true, Type=\"Text\"]")
                .as_deref(),
            Some("C:\\\\data\\\\")
        );
    }
}
