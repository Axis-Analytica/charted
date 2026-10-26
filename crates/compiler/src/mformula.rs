//! Custom column formulas: a subset of Power Query's M expression language, so a
//! formula means the same thing in charted and in an exported Power BI project.
//!
//! ```text
//! [Quantity] * [Unit Price]
//! if [Region] = "EU" then "Europe" else Text.Upper([Region])
//! Text.Start([Code], 3) & "-" & Text.From([Year])
//! ```
//!
//! Columns are `[Name]`; literals are numbers, "text" (`""` escapes a quote), `true`,
//! `false` and `null`; operators are `+ - * / &`, comparisons `= <> < <= > >=`,
//! `and`/`or`/`not` and `if … then … else …`. The functions are listed in `FUNCTIONS`.

use crate::quote;

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Column(String),
    Number(f64),
    Text(String),
    Bool(bool),
    Null,
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Binary(Box<Expr>, Op, Box<Expr>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    Call(&'static str, Vec<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Concat,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

/// M function, number of arguments (min, max), SQL template (`$1`, `$2`…).
pub const FUNCTIONS: &[(&str, usize, usize, &str)] = &[
    ("Text.Upper", 1, 1, "upper($1)"),
    ("Text.Lower", 1, 1, "lower($1)"),
    ("Text.Trim", 1, 1, "trim($1)"),
    ("Text.Length", 1, 1, "length($1)"),
    ("Text.Start", 2, 2, "left($1, $2)"),
    ("Text.End", 2, 2, "right($1, $2)"),
    ("Text.Middle", 3, 3, "substr($1, ($2) + 1, $3)"),
    ("Text.Contains", 2, 2, "contains($1, $2)"),
    ("Text.Replace", 3, 3, "replace($1, $2, $3)"),
    ("Text.From", 1, 1, "CAST($1 AS VARCHAR)"),
    ("Number.From", 1, 1, "TRY_CAST($1 AS DOUBLE)"),
    ("Number.Round", 1, 2, "round($1, $2)"),
    ("Number.Abs", 1, 1, "abs($1)"),
    ("Date.Year", 1, 1, "year($1)"),
    ("Date.Month", 1, 1, "month($1)"),
    ("Date.Day", 1, 1, "day($1)"),
    ("Date.QuarterOfYear", 1, 1, "quarter($1)"),
    // The day of the week from 0, the week starting on Sunday unless `Day.…` says.
    ("Date.DayOfWeek", 1, 2, "((dayofweek($1) - ($2) + 7) % 7)"),
    // Week 1 holds 1 January; weeks start on Sunday unless `Day.…` says.
    (
        "Date.WeekOfYear",
        1,
        2,
        "((dayofyear($1) - 1 + (dayofweek(date_trunc('year', $1)) - ($2) + 7) % 7) // 7 + 1)",
    ),
    // English names, whatever the culture.
    ("Date.MonthName", 1, 2, "monthname($1)"),
    ("Date.DayOfWeekName", 1, 2, "dayname($1)"),
    // The format is a .NET date format ("yyyy-MM", "MMM", "dddd"), see `date_format`.
    ("Date.ToText", 1, 3, "CAST($1 AS VARCHAR)"),
    (
        "Number.IntegerDivide",
        2,
        2,
        "CAST(trunc(($1) / ($2)) AS BIGINT)",
    ),
];

/// Power Query's days of the week (`Day.Monday`), as the numbers M gives them.
const DAYS: [&str; 7] = [
    "Day.Sunday",
    "Day.Monday",
    "Day.Tuesday",
    "Day.Wednesday",
    "Day.Thursday",
    "Day.Friday",
    "Day.Saturday",
];

/// A .NET custom date format ("yyyy-MM", "MMM d") as a DuckDB strftime format, or
/// None for one it can't write (standard one-letter formats such as "d").
pub fn date_format(f: &str) -> Option<String> {
    if f.chars().count() == 1 {
        return None;
    }
    let cs: Vec<char> = f.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        let mut n = 1;
        while i + n < cs.len() && cs[i + n] == c {
            n += 1;
        }
        let spec = match (c, n) {
            ('y', 1 | 2) => "%y",
            ('y', _) => "%Y",
            ('M', 1) => "%-m",
            ('M', 2) => "%m",
            ('M', 3) => "%b",
            ('M', _) => "%B",
            ('d', 1) => "%-d",
            ('d', 2) => "%d",
            ('d', 3) => "%a",
            ('d', _) => "%A",
            ('H', 1) => "%-H",
            ('H', _) => "%H",
            ('h', 1) => "%-I",
            ('h', _) => "%I",
            ('m', 1) => "%-M",
            ('m', _) => "%M",
            ('s', 1) => "%-S",
            ('s', _) => "%S",
            ('t', _) => "%p",
            ('\'' | '"', _) => {
                // A quoted literal.
                let end = cs[i + 1..].iter().position(|x| *x == c)?;
                for x in &cs[i + 1..i + 1 + end] {
                    if *x == '%' {
                        out.push('%');
                    }
                    out.push(*x);
                }
                i += end + 2;
                continue;
            }
            ('\\', _) => {
                let x = *cs.get(i + 1)?;
                if x == '%' {
                    out.push('%');
                }
                out.push(x);
                i += 2;
                continue;
            }
            ('%', _) => {
                out.push_str(&"%%".repeat(n));
                i += n;
                continue;
            }
            (c, _) if c.is_ascii_alphabetic() => return None,
            _ => {
                out.extend(std::iter::repeat_n(c, n));
                i += n;
                continue;
            }
        };
        out.push_str(spec);
        i += n;
    }
    Some(out)
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{0}")]
pub struct FormulaError(pub String);

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Col(String),
    Ident(String),
    Sym(&'static str),
}

fn lex(src: &str) -> Result<Vec<Tok>, FormulaError> {
    let cs: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '[' {
            let start = i + 1;
            let end = cs[start..]
                .iter()
                .position(|&x| x == ']')
                .ok_or_else(|| FormulaError("a column name is missing its closing ]".into()))?;
            out.push(Tok::Col(cs[start..start + end].iter().collect()));
            i = start + end + 1;
        } else if c == '"' {
            let mut s = String::new();
            i += 1;
            loop {
                match cs.get(i) {
                    None => {
                        return Err(FormulaError(
                            "a text value is missing its closing \"".into(),
                        ));
                    }
                    Some('"') if cs.get(i + 1) == Some(&'"') => {
                        s.push('"');
                        i += 2;
                    }
                    Some('"') => {
                        i += 1;
                        break;
                    }
                    Some(&ch) => {
                        s.push(ch);
                        i += 1;
                    }
                }
            }
            out.push(Tok::Str(s));
        } else if c.is_ascii_digit()
            || (c == '.' && cs.get(i + 1).is_some_and(|d| d.is_ascii_digit()))
        {
            let start = i;
            while i < cs.len() && (cs[i].is_ascii_digit() || cs[i] == '.') {
                i += 1;
            }
            let t: String = cs[start..i].iter().collect();
            out.push(Tok::Num(
                t.parse()
                    .map_err(|_| FormulaError(format!("\"{t}\" isn't a number")))?,
            ));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_' || cs[i] == '.') {
                i += 1;
            }
            out.push(Tok::Ident(cs[start..i].iter().collect()));
        } else {
            let two: String = cs[i..(i + 2).min(cs.len())].iter().collect();
            let sym = ["<>", "<=", ">="].into_iter().find(|s| *s == two);
            if let Some(s) = sym {
                out.push(Tok::Sym(s));
                i += 2;
            } else {
                let s = ["+", "-", "*", "/", "&", "=", "<", ">", "(", ")", ","]
                    .into_iter()
                    .find(|s| s.starts_with(c))
                    .ok_or_else(|| FormulaError(format!("unexpected \"{c}\"")))?;
                out.push(Tok::Sym(s));
                i += 1;
            }
        }
    }
    Ok(out)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn is_word(&self, w: &str) -> bool {
        matches!(self.peek(), Some(Tok::Ident(x)) if x == w)
    }
    fn is_sym(&self, s: &str) -> bool {
        matches!(self.peek(), Some(Tok::Sym(x)) if *x == s)
    }
    fn expect_word(&mut self, w: &str) -> Result<(), FormulaError> {
        if self.is_word(w) {
            self.pos += 1;
            Ok(())
        } else {
            Err(FormulaError(format!("expected \"{w}\"")))
        }
    }
    fn expect_sym(&mut self, s: &str) -> Result<(), FormulaError> {
        if self.is_sym(s) {
            self.pos += 1;
            Ok(())
        } else {
            Err(FormulaError(format!("expected \"{s}\"")))
        }
    }

    fn expr(&mut self) -> Result<Expr, FormulaError> {
        if self.is_word("if") {
            self.pos += 1;
            let c = self.expr()?;
            self.expect_word("then")?;
            let a = self.expr()?;
            self.expect_word("else")?;
            let b = self.expr()?;
            return Ok(Expr::If(Box::new(c), Box::new(a), Box::new(b)));
        }
        self.or()
    }
    fn or(&mut self) -> Result<Expr, FormulaError> {
        let mut l = self.and()?;
        while self.is_word("or") {
            self.pos += 1;
            l = Expr::Binary(Box::new(l), Op::Or, Box::new(self.and()?));
        }
        Ok(l)
    }
    fn and(&mut self) -> Result<Expr, FormulaError> {
        let mut l = self.not()?;
        while self.is_word("and") {
            self.pos += 1;
            l = Expr::Binary(Box::new(l), Op::And, Box::new(self.not()?));
        }
        Ok(l)
    }
    fn not(&mut self) -> Result<Expr, FormulaError> {
        if self.is_word("not") {
            self.pos += 1;
            return Ok(Expr::Not(Box::new(self.not()?)));
        }
        self.cmp()
    }
    fn cmp(&mut self) -> Result<Expr, FormulaError> {
        let l = self.concat()?;
        let op = match self.peek() {
            Some(Tok::Sym("=")) => Op::Eq,
            Some(Tok::Sym("<>")) => Op::Ne,
            Some(Tok::Sym("<")) => Op::Lt,
            Some(Tok::Sym("<=")) => Op::Le,
            Some(Tok::Sym(">")) => Op::Gt,
            Some(Tok::Sym(">=")) => Op::Ge,
            _ => return Ok(l),
        };
        self.pos += 1;
        Ok(Expr::Binary(Box::new(l), op, Box::new(self.concat()?)))
    }
    fn concat(&mut self) -> Result<Expr, FormulaError> {
        let mut l = self.add()?;
        while self.is_sym("&") {
            self.pos += 1;
            l = Expr::Binary(Box::new(l), Op::Concat, Box::new(self.add()?));
        }
        Ok(l)
    }
    fn add(&mut self) -> Result<Expr, FormulaError> {
        let mut l = self.mul()?;
        loop {
            let op = if self.is_sym("+") {
                Op::Add
            } else if self.is_sym("-") {
                Op::Sub
            } else {
                return Ok(l);
            };
            self.pos += 1;
            l = Expr::Binary(Box::new(l), op, Box::new(self.mul()?));
        }
    }
    fn mul(&mut self) -> Result<Expr, FormulaError> {
        let mut l = self.unary()?;
        loop {
            let op = if self.is_sym("*") {
                Op::Mul
            } else if self.is_sym("/") {
                Op::Div
            } else {
                return Ok(l);
            };
            self.pos += 1;
            l = Expr::Binary(Box::new(l), op, Box::new(self.unary()?));
        }
    }
    fn unary(&mut self) -> Result<Expr, FormulaError> {
        if self.is_sym("-") {
            self.pos += 1;
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        self.primary()
    }
    fn primary(&mut self) -> Result<Expr, FormulaError> {
        let t = self
            .peek()
            .cloned()
            .ok_or_else(|| FormulaError("the formula ends too soon".into()))?;
        self.pos += 1;
        match t {
            Tok::Num(n) => Ok(Expr::Number(n)),
            Tok::Str(s) => Ok(Expr::Text(s)),
            Tok::Col(c) => Ok(Expr::Column(c)),
            Tok::Sym("(") => {
                let e = self.expr()?;
                self.expect_sym(")")?;
                Ok(e)
            }
            Tok::Ident(w) => match w.as_str() {
                "true" => Ok(Expr::Bool(true)),
                "false" => Ok(Expr::Bool(false)),
                "null" => Ok(Expr::Null),
                w if DAYS.contains(&w) => Ok(Expr::Number(
                    DAYS.iter().position(|d| *d == w).unwrap_or(0) as f64,
                )),
                _ => {
                    let (name, min, max, _) =
                        FUNCTIONS.iter().find(|f| f.0 == w).ok_or_else(|| {
                            FormulaError(format!("{w} isn't a function charted knows"))
                        })?;
                    self.expect_sym("(")?;
                    let mut args = Vec::new();
                    if !self.is_sym(")") {
                        loop {
                            args.push(self.expr()?);
                            if self.is_sym(",") {
                                self.pos += 1;
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect_sym(")")?;
                    if args.len() < *min || args.len() > *max {
                        return Err(FormulaError(format!(
                            "{name} takes {}",
                            if min == max {
                                format!("{min} argument{}", if *min == 1 { "" } else { "s" })
                            } else {
                                format!("{min} to {max} arguments")
                            }
                        )));
                    }
                    if *name == "Date.ToText" {
                        match args.get(1) {
                            None => {}
                            Some(Expr::Text(f)) if date_format(f).is_some() => {}
                            Some(Expr::Text(f)) => {
                                return Err(FormulaError(format!(
                                    "the date format \"{f}\" isn't one charted can write"
                                )));
                            }
                            Some(_) => {
                                return Err(FormulaError(
                                    "Date.ToText's format must be text, such as \"yyyy-MM\"".into(),
                                ));
                            }
                        }
                    }
                    Ok(Expr::Call(name, args))
                }
            },
            Tok::Sym(s) => Err(FormulaError(format!("unexpected \"{s}\""))),
        }
    }
}

/// Parses a formula, checking that the columns it uses are among `columns`.
pub fn parse(src: &str, columns: &[String]) -> Result<Expr, FormulaError> {
    let mut p = Parser {
        toks: lex(src)?,
        pos: 0,
    };
    if p.toks.is_empty() {
        return Err(FormulaError("write a formula".into()));
    }
    let e = p.expr()?;
    if p.pos < p.toks.len() {
        return Err(FormulaError(
            "there is more after the end of the formula".into(),
        ));
    }
    let mut missing = None;
    walk(&e, &mut |x| {
        if let Expr::Column(c) = x
            && !columns.is_empty()
            && !columns.contains(c)
        {
            missing.get_or_insert(c.clone());
        }
    });
    match missing {
        Some(c) => Err(FormulaError(format!("there is no column [{c}]"))),
        None => Ok(e),
    }
}

fn walk(e: &Expr, f: &mut impl FnMut(&Expr)) {
    f(e);
    match e {
        Expr::Neg(x) | Expr::Not(x) => walk(x, f),
        Expr::Binary(a, _, b) => {
            walk(a, f);
            walk(b, f);
        }
        Expr::If(a, b, c) => {
            walk(a, f);
            walk(b, f);
            walk(c, f);
        }
        Expr::Call(_, args) => args.iter().for_each(|a| walk(a, f)),
        _ => {}
    }
}

fn number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

pub fn to_sql(e: &Expr) -> String {
    match e {
        Expr::Column(c) => quote::ident(c),
        Expr::Number(n) => number(*n),
        Expr::Text(s) => quote::string(s),
        Expr::Bool(b) => b.to_string().to_uppercase(),
        Expr::Null => "NULL".into(),
        Expr::Neg(x) => format!("(-{})", to_sql(x)),
        Expr::Not(x) => format!("(NOT {})", to_sql(x)),
        Expr::Binary(a, op, b) => {
            let (a, b) = (to_sql(a), to_sql(b));
            match op {
                Op::Div => format!("(CAST({a} AS DOUBLE) / {b})"),
                Op::Concat => format!("(CAST({a} AS VARCHAR) || CAST({b} AS VARCHAR))"),
                _ => {
                    let s = match op {
                        Op::Add => "+",
                        Op::Sub => "-",
                        Op::Mul => "*",
                        Op::Eq => "=",
                        Op::Ne => "<>",
                        Op::Lt => "<",
                        Op::Le => "<=",
                        Op::Gt => ">",
                        Op::Ge => ">=",
                        Op::And => "AND",
                        Op::Or => "OR",
                        Op::Div | Op::Concat => unreachable!(),
                    };
                    format!("({a} {s} {b})")
                }
            }
        }
        Expr::If(c, a, b) => format!(
            "(CASE WHEN {} THEN {} ELSE {} END)",
            to_sql(c),
            to_sql(a),
            to_sql(b)
        ),
        Expr::Call("Date.ToText", args) if args.len() > 1 => {
            let f = match &args[1] {
                Expr::Text(f) => date_format(f).unwrap_or_default(),
                _ => String::new(),
            };
            format!("strftime({}, {})", to_sql(&args[0]), quote::string(&f))
        }
        Expr::Call(name, args) => {
            let (_, _, _, tpl) = FUNCTIONS.iter().find(|f| f.0 == *name).expect("known");
            let mut out = tpl.to_string();
            // Optional trailing arguments default to 0 (Number.Round's digits).
            for i in (1..=3).rev() {
                let v = args.get(i - 1).map(to_sql).unwrap_or_else(|| "0".into());
                out = out.replace(&format!("${i}"), &v);
            }
            out
        }
    }
}

fn m_string(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// The formula as M (for `Table.AddColumn(…, each <formula>)`).
pub fn to_m(e: &Expr) -> String {
    match e {
        Expr::Column(c) => format!("[{c}]"),
        Expr::Number(n) => number(*n),
        Expr::Text(s) => m_string(s),
        Expr::Bool(b) => b.to_string(),
        Expr::Null => "null".into(),
        Expr::Neg(x) => format!("-{}", to_m(x)),
        Expr::Not(x) => format!("not {}", to_m(x)),
        Expr::Binary(a, op, b) => {
            let s = match op {
                Op::Add => "+",
                Op::Sub => "-",
                Op::Mul => "*",
                Op::Div => "/",
                Op::Concat => "&",
                Op::Eq => "=",
                Op::Ne => "<>",
                Op::Lt => "<",
                Op::Le => "<=",
                Op::Gt => ">",
                Op::Ge => ">=",
                Op::And => "and",
                Op::Or => "or",
            };
            format!("({} {s} {})", to_m(a), to_m(b))
        }
        Expr::If(c, a, b) => format!("if {} then {} else {}", to_m(c), to_m(a), to_m(b)),
        Expr::Call(name, args) => format!(
            "{name}({})",
            args.iter().map(to_m).collect::<Vec<_>>().join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols() -> Vec<String> {
        ["Qty", "Price", "Region", "Code"]
            .map(String::from)
            .to_vec()
    }

    #[test]
    fn parses_and_emits_sql_and_m() {
        let e = parse("[Qty] * [Price] + 1", &cols()).unwrap();
        assert_eq!(to_sql(&e), r#"(("Qty" * "Price") + 1)"#);
        assert_eq!(to_m(&e), "(([Qty] * [Price]) + 1)");
        let e = parse(
            r#"if [Region] = "EU" then "Europe" else Text.Upper([Region])"#,
            &cols(),
        )
        .unwrap();
        assert_eq!(
            to_sql(&e),
            r#"(CASE WHEN ("Region" = 'EU') THEN 'Europe' ELSE upper("Region") END)"#
        );
        assert_eq!(
            to_m(&e),
            r#"if ([Region] = "EU") then "Europe" else Text.Upper([Region])"#
        );
        let e = parse(r#"Text.Start([Code], 3) & "-""x""" "#, &cols()).unwrap();
        assert_eq!(to_m(&e), r#"(Text.Start([Code], 3) & "-""x""")"#);
        assert_eq!(
            to_sql(&parse("Number.Round([Price])", &cols()).unwrap()),
            r#"round("Price", 0)"#
        );
    }

    #[test]
    fn date_functions_of_calendars() {
        let sql = |s: &str| to_sql(&parse(s, &[]).unwrap());
        assert_eq!(
            sql("Date.ToText([Date], \"yyyy-MM 'Q'\")"),
            "strftime(\"Date\", '%Y-%m Q')"
        );
        assert_eq!(
            sql("Date.ToText([Date], \"dddd d MMMM\")"),
            "strftime(\"Date\", '%A %-d %B')"
        );
        assert_eq!(
            sql("Date.DayOfWeek([Date], Day.Monday) + 1"),
            "(((dayofweek(\"Date\") - (1) + 7) % 7) + 1)"
        );
        assert_eq!(
            sql("Date.DayOfWeek([Date])"),
            "((dayofweek(\"Date\") - (0) + 7) % 7)"
        );
        assert_eq!(
            to_m(&parse("Date.DayOfWeek([Date], Day.Monday)", &[]).unwrap()),
            "Date.DayOfWeek([Date], 1)"
        );
        assert!(
            parse("Date.ToText([Date], \"d\")", &[])
                .unwrap_err()
                .0
                .contains("isn't one charted can write")
        );
        assert_eq!(date_format("100%"), Some("100%%".into()));
    }

    #[test]
    fn explains_mistakes() {
        for (src, msg) in [
            ("[Qty", "closing ]"),
            ("[Nope] + 1", "no column [Nope]"),
            ("Text.Upper([Qty], 2)", "takes 1 argument"),
            ("Foo.Bar(1)", "isn't a function"),
            ("1 +", "ends too soon"),
            ("if [Qty] then 1", "expected \"else\""),
            ("", "write a formula"),
        ] {
            let e = parse(src, &cols()).unwrap_err();
            assert!(e.0.contains(msg), "{src}: {}", e.0);
        }
    }
}
