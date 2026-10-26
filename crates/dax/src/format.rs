//! The FORMAT format strings charted evaluates. The binder parses a literal format
//! string into a `FormatSpec`; the SQL compiler and the interpreter each render it, so
//! anything outside this small, well-defined subset is refused instead of guessed.

/// A parsed FORMAT format string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatSpec {
    /// A number rounded (halves away from zero) to `decimals` places, with thousands
    /// separators, as a percentage (times 100, then "%"), after a currency symbol.
    Number {
        decimals: usize,
        thousands: bool,
        percent: bool,
        prefix: &'static str,
    },
    /// "General Number": the number with as many digits as it needs.
    General,
    /// A date written part by part.
    Date(Vec<DatePart>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatePart {
    /// yyyy
    Year,
    /// yy
    Year2,
    /// mmmm: January
    MonthName,
    /// mmm: Jan
    MonthAbbr,
    /// mm: 01
    Month2,
    /// m: 1
    Month,
    /// dddd: Monday
    DayName,
    /// ddd: Mon
    DayAbbr,
    /// dd: 05
    Day2,
    /// d: 5
    Day,
    /// Separators written as they are.
    Literal(String),
}

pub const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

pub const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// Parses a format string, or explains which ones are supported.
pub fn parse(s: &str) -> Result<FormatSpec, String> {
    let named = match s.to_ascii_lowercase().as_str() {
        "general number" => Some(FormatSpec::General),
        "percent" => Some(number(2, false, true, "")),
        "fixed" => Some(number(2, false, false, "")),
        "standard" => Some(number(2, true, false, "")),
        _ => None,
    };
    if let Some(n) = named {
        return Ok(n);
    }
    number_pattern(s)
        .or_else(|| date_pattern(s))
        .ok_or_else(|| {
            format!(
                "the format string \"{s}\" isn't supported yet; use a number format such as \"0\", \"0.00\", \"#,0\", \"$#,0.00\" or \"0.0%\", a date format such as \"yyyy-MM-dd\", \"MMM yyyy\" or \"dd/mm/yyyy\", or \"General Number\" or \"Percent\""
            )
        })
}

fn number(decimals: usize, thousands: bool, percent: bool, prefix: &'static str) -> FormatSpec {
    FormatSpec::Number {
        decimals,
        thousands,
        percent,
        prefix,
    }
}

/// `[currency]0`, `#,0` or `#,##0`, then optional `.000…`, then optional `%`.
fn number_pattern(s: &str) -> Option<FormatSpec> {
    let (prefix, rest) = ["$", "€", "£", "¥"]
        .iter()
        .find_map(|p| s.strip_prefix(p).map(|r| (*p, r)))
        .unwrap_or(("", s));
    let (rest, percent) = match rest.strip_suffix('%') {
        Some(r) => (r, true),
        None => (rest, false),
    };
    let (int, frac) = match rest.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (rest, None),
    };
    let thousands = match int {
        "0" => false,
        "#,0" | "#,##0" => true,
        _ => return None,
    };
    let decimals = match frac {
        None => 0,
        Some(f) if !f.is_empty() && f.chars().all(|c| c == '0') => f.len(),
        Some(_) => return None,
    };
    Some(number(decimals, thousands, percent, prefix))
}

/// Day, month and year codes (case-insensitive, as m means month without hours) and
/// the separators `-`, `/`, `.`, `,` and space.
fn date_pattern(s: &str) -> Option<FormatSpec> {
    let lower = s.to_ascii_lowercase();
    let b = lower.as_bytes();
    let mut parts: Vec<DatePart> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let run = b[i..].iter().take_while(|&&x| x == c).count();
        let part = match (c, run) {
            (b'y', 4) => DatePart::Year,
            (b'y', 2) => DatePart::Year2,
            (b'm', 4) => DatePart::MonthName,
            (b'm', 3) => DatePart::MonthAbbr,
            (b'm', 2) => DatePart::Month2,
            (b'm', 1) => DatePart::Month,
            (b'd', 4) => DatePart::DayName,
            (b'd', 3) => DatePart::DayAbbr,
            (b'd', 2) => DatePart::Day2,
            (b'd', 1) => DatePart::Day,
            (b'-' | b'/' | b'.' | b',' | b' ', _) => {
                let lit = &s[i..i + run];
                match parts.last_mut() {
                    Some(DatePart::Literal(l)) => l.push_str(lit),
                    _ => parts.push(DatePart::Literal(lit.to_string())),
                }
                i += run;
                continue;
            }
            _ => return None,
        };
        parts.push(part);
        i += run;
    }
    parts
        .iter()
        .any(|p| !matches!(p, DatePart::Literal(_)))
        .then_some(FormatSpec::Date(parts))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_supported_formats() {
        assert_eq!(parse("0"), Ok(number(0, false, false, "")));
        assert_eq!(parse("#,##0.00"), Ok(number(2, true, false, "")));
        assert_eq!(parse("0.0%"), Ok(number(1, false, true, "")));
        assert_eq!(parse("$#,0"), Ok(number(0, true, false, "$")));
        assert_eq!(parse("Percent"), Ok(number(2, false, true, "")));
        assert_eq!(parse("general number"), Ok(FormatSpec::General));
        assert_eq!(
            parse("MMM yyyy"),
            Ok(FormatSpec::Date(vec![
                DatePart::MonthAbbr,
                DatePart::Literal(" ".into()),
                DatePart::Year
            ]))
        );
        assert!(matches!(parse("dd/mm/yyyy"), Ok(FormatSpec::Date(p)) if p.len() == 5));
        for bad in [
            "0.#",
            "#",
            "yyy",
            "hh:nn",
            "Short Date",
            "0.00 \"kg\"",
            "",
            "--",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
}
