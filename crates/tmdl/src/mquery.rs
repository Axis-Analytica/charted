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
        Tok::Ident(w) if w == "type" => match l.next() {
            Tok::Ident(t) => M::Type(t),
            t => return Err(format!("unexpected {t:?} after type")),
        },
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
                let name = match l.next() {
                    Tok::Ident(n) | Tok::Str(n) => n,
                    t => return Err(format!("unexpected {t:?} in a field access")),
                };
                expect(l, ']')?;
                e = M::Field(Box::new(e), name);
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

fn expr(l: &mut Lexer) -> Result<M, String> {
    let mut e = primary(l)?;
    while l.peek() == Tok::Sym('&') {
        l.next();
        e = M::Concat(Box::new(e), Box::new(primary(l)?));
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
    let mut steps = Vec::new();
    loop {
        let name = match l.next() {
            Tok::Ident(n) => n,
            t => return Err(format!("unexpected {t:?} where a step name belongs")),
        };
        expect(&mut l, '=')?;
        steps.push((name, expr(&mut l)?));
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
    let result = match l.next() {
        Tok::Ident(n) => n,
        t => return Err(format!("unexpected {t:?} after in")),
    };
    Ok((steps, result))
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

fn count(m: &M) -> Option<u32> {
    match m {
        M::Num(n) if *n >= 0.0 && n.fract() == 0.0 => Some(*n as u32),
        _ => None,
    }
}

/// A source and steps for an M query that loads a file, or why it can't be.
pub fn to_query(
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
    for (name, e) in &steps[..=end] {
        let call = match e {
            M::Call(f, args) => Some((name_of(f), args.as_slice())),
            _ => None,
        };
        // The first argument of a table function must be the previous step.
        let on_prev =
            |args: &[M]| matches!(args.first(), Some(M::Ident(p)) if Some(p.as_str()) == prev);
        match (&source, call, e) {
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
                    out.push(Step::ChangeTypes { columns });
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
