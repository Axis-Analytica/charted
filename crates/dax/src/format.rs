//! The FORMAT format strings charted evaluates. The binder parses a literal format
//! string into a `FormatSpec`; the SQL compiler and the interpreter each render it, so
//! anything outside this small, well-defined subset is refused instead of guessed.

/// A number as DAX writes it as text (`"" & x`, FORMAT's "General Number"), as the
/// Power BI Service answered: rounded to 15 significant digits, in fixed notation
/// unless that needs more than 15 digits before the point or after it, then as
/// `3.33333333333333E-05`. The SQL compiler's `general_number_sql` is the same rule.
pub fn general_number(x: f64) -> String {
    if x == 0.0 {
        return "0".into();
    }
    // As the Service writes them (its C runtime's spelling).
    if !x.is_finite() {
        return if x.is_nan() {
            "-nan(ind)"
        } else if x > 0.0 {
            "inf"
        } else {
            "-inf"
        }
        .into();
    }
    // Rounded to 15 significant digits first, halves away from zero as Power BI does
    // (`{:.14e}` alone would round a tie to even); the SQL does the same with round().
    let s = 14 - x.abs().log10().floor() as i32;
    let r = if (-300..=300).contains(&s) {
        let m = 10f64.powi(s.abs());
        if s >= 0 {
            (x.abs() * m).round() / m
        } else {
            (x.abs() / m).round() * m
        }
    } else {
        x.abs()
    };
    let sci = format!("{:.14e}", r);
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let e: i32 = exp.parse().unwrap_or(0);
    let digits = mantissa.replace('.', "");
    let digits = digits.trim_end_matches('0');
    let k = digits.len() as i32;
    let sign = if x < 0.0 { "-" } else { "" };
    let body = if e < 15 && (e >= 0 || -e - 1 + k <= 15) {
        if e >= 0 {
            let int = (e + 1) as usize;
            if digits.len() <= int {
                format!("{digits}{}", "0".repeat(int - digits.len()))
            } else {
                format!("{}.{}", &digits[..int], &digits[int..])
            }
        } else {
            format!("0.{}{digits}", "0".repeat((-e - 1) as usize))
        }
    } else {
        let rest = if k > 1 {
            format!(".{}", &digits[1..])
        } else {
            String::new()
        };
        let sign = if e < 0 { '-' } else { '+' };
        format!("{}{rest}E{sign}{:02}", &digits[..1], e.abs())
    };
    format!("{sign}{body}")
}

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
    }

    #[test]
    fn general_number_matches_power_bi() {
        // Each as the Power BI Service wrote it (November 2026).
        for (x, want) in [
            (1.0 / 3.0, "0.333333333333333"),
            (87.03717472118959, "87.0371747211896"),
            (1234567.891, "1234567.891"),
            (0.000012345, "0.000012345"),
            (1e20 / 3.0, "3.33333333333333E+19"),
            (1e15, "1E+15"),
            (123456789012345.0, "123456789012345"),
            (100000000000000.5, "100000000000001"),
            (0.0001, "0.0001"),
            (1e-10 / 3.0, "3.33333333333333E-11"),
            (-2.5, "-2.5"),
            (1e-5, "0.00001"),
            (0.00001234567, "0.00001234567"),
            (1.0 / 3.0 * 1e-4, "3.33333333333333E-05"),
            (3.33333e-5, "0.0000333333"),
            (1e-5 / 3.0, "3.33333333333333E-06"),
            (1.0 / 3.0 * 1e-3, "3.33333333333333E-04"),
            (0.00012345, "0.00012345"),
            (0.1 + 0.2, "0.3"),
            (123456.5, "123456.5"),
            (0.0, "0"),
            (-0.0, "0"),
        ] {
            assert_eq!(general_number(x), want, "{x:e}");
        }
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
