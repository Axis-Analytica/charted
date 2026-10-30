//! Fields by name, as the agent writes them: `'Table'[Column]`, `Table[Column]`, a measure
//! as `[Measure]` (or with its table), and a column summarised in a value role as
//! `SUM('Sales'[Amount])` (SUM, AVERAGE, MIN, MAX, COUNT, DISTINCTCOUNT).

use charted_model::{Aggregation, FieldRef, Model, Page, Projection, Table, Visual};

/// A field as written, before it is looked up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSpec {
    pub table: Option<String>,
    pub name: String,
    pub aggregation: Option<Aggregation>,
}

/// Why a name didn't resolve, with the closest names there are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameError {
    pub message: String,
    pub suggestions: Vec<String>,
}

impl std::fmt::Display for NameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)?;
        if !self.suggestions.is_empty() {
            write!(f, " Did you mean {}?", self.suggestions.join(", "))?;
        }
        Ok(())
    }
}

fn err(message: impl Into<String>, suggestions: Vec<String>) -> NameError {
    NameError {
        message: message.into(),
        suggestions,
    }
}

const AGGREGATIONS: &[(&str, Aggregation)] = &[
    ("SUM", Aggregation::Sum),
    ("AVERAGE", Aggregation::Average),
    ("AVG", Aggregation::Average),
    ("MIN", Aggregation::Min),
    ("MAX", Aggregation::Max),
    ("COUNT", Aggregation::Count),
    ("DISTINCTCOUNT", Aggregation::DistinctCount),
];

pub fn aggregation_name(a: Aggregation) -> &'static str {
    match a {
        Aggregation::Sum => "SUM",
        Aggregation::Average => "AVERAGE",
        Aggregation::Min => "MIN",
        Aggregation::Max => "MAX",
        Aggregation::Count => "COUNT",
        Aggregation::DistinctCount => "DISTINCTCOUNT",
    }
}

/// Reads a field reference.
pub fn parse_field(src: &str) -> Result<FieldSpec, NameError> {
    let s = src.trim();
    // An aggregation around a column.
    if let Some(open) = s.find('(')
        && s.ends_with(')')
        && !s[..open].contains(['[', '\''])
    {
        let func = s[..open].trim().to_ascii_uppercase();
        let Some((_, agg)) = AGGREGATIONS.iter().find(|(n, _)| *n == func) else {
            return Err(err(
                format!(
                    "{src}: only SUM, AVERAGE, MIN, MAX, COUNT and DISTINCTCOUNT can wrap a field here; write other expressions as a measure."
                ),
                vec![],
            ));
        };
        let mut inner = parse_field(&s[open + 1..s.len() - 1])?;
        if inner.aggregation.is_some() {
            return Err(err(format!("{src}: one aggregation only."), vec![]));
        }
        inner.aggregation = Some(*agg);
        return Ok(inner);
    }
    let (table, rest) = if let Some(after) = s.strip_prefix('\'') {
        // 'Table' with '' for a quote.
        let mut name = String::new();
        let mut chars = after.char_indices().peekable();
        let mut end = None;
        while let Some((i, c)) = chars.next() {
            if c == '\'' {
                if matches!(chars.peek(), Some((_, '\''))) {
                    chars.next();
                    name.push('\'');
                } else {
                    end = Some(i + 1);
                    break;
                }
            } else {
                name.push(c);
            }
        }
        let Some(end) = end else {
            return Err(err(
                format!("{src}: the table name's quote isn't closed."),
                vec![],
            ));
        };
        (Some(name), after[end..].trim_start())
    } else if let Some(i) = s.find('[') {
        let t = s[..i].trim();
        (
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            },
            &s[i..],
        )
    } else {
        return Err(err(
            format!("{src}: write a field as 'Table'[Column] or [Measure]."),
            vec![],
        ));
    };
    let Some(body) = rest.strip_prefix('[') else {
        return Err(err(
            format!("{src}: expected [Name] after the table."),
            vec![],
        ));
    };
    let Some(close) = closing_bracket(body) else {
        return Err(err(format!("{src}: the [ isn't closed."), vec![]));
    };
    if !body[close + 1..].trim().is_empty() {
        return Err(err(
            format!("{src}: unexpected text after the field."),
            vec![],
        ));
    }
    Ok(FieldSpec {
        table,
        name: body[..close].replace("]]", "]"),
        aggregation: None,
    })
}

/// The index of the `]` that closes a name (`]]` is a literal bracket).
fn closing_bracket(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b']' {
            if b.get(i + 1) == Some(&b']') {
                i += 2;
                continue;
            }
            return Some(i);
        }
        i += 1;
    }
    None
}

/// `'Table'`, quoted for DAX.
pub fn quote_table(name: &str) -> String {
    format!("'{}'", name.replace('\'', "''"))
}

/// `[Name]`, quoted for DAX.
pub fn quote_name(name: &str) -> String {
    format!("[{}]", name.replace(']', "]]"))
}

/// A table by name.
pub fn find_table<'a>(model: &'a Model, name: &str) -> Result<&'a Table, NameError> {
    let name = name.trim().trim_matches('\'');
    model.table_by_name(name).ok_or_else(|| {
        err(
            format!("There's no table {}.", quote_table(name)),
            closest(name, model.tables.iter().map(|t| quote_table(&t.name))),
        )
    })
}

/// A field by reference. A measure is found in any table when none is named; a name
/// two tables' measures share must be given its table.
pub fn resolve(model: &Model, spec: &FieldSpec) -> Result<Projection, NameError> {
    let field = match &spec.table {
        Some(tn) => {
            let t = find_table(model, tn)?;
            if let Some(c) = t.column_by_name(&spec.name) {
                FieldRef::Column {
                    table: t.id.clone(),
                    column: c.id.clone(),
                }
            } else if let Some(m) = t
                .measures
                .iter()
                .find(|m| m.name.eq_ignore_ascii_case(&spec.name))
            {
                FieldRef::Measure {
                    table: t.id.clone(),
                    measure: m.id.clone(),
                }
            } else {
                return Err(err(
                    format!(
                        "{} has no column or measure {}.",
                        quote_table(&t.name),
                        quote_name(&spec.name)
                    ),
                    closest(
                        &spec.name,
                        t.columns
                            .iter()
                            .map(|c| &c.name)
                            .chain(t.measures.iter().map(|m| &m.name))
                            .map(|n| format!("{}{}", quote_table(&t.name), quote_name(n))),
                    ),
                ));
            }
        }
        None => {
            let found: Vec<_> = model
                .tables
                .iter()
                .flat_map(|t| t.measures.iter().map(move |m| (t, m)))
                .filter(|(_, m)| m.name.eq_ignore_ascii_case(&spec.name))
                .collect();
            match found.as_slice() {
                [(t, m)] => FieldRef::Measure {
                    table: t.id.clone(),
                    measure: m.id.clone(),
                },
                [] => {
                    // A column named without its table, when only one table has it.
                    let cols: Vec<_> = model
                        .tables
                        .iter()
                        .filter_map(|t| t.column_by_name(&spec.name).map(|c| (t, c)))
                        .collect();
                    let names = |v: &[(&Table, &charted_model::Column)]| {
                        v.iter()
                            .map(|(t, c)| {
                                format!("{}{}", quote_table(&t.name), quote_name(&c.name))
                            })
                            .collect::<Vec<_>>()
                    };
                    if !cols.is_empty() {
                        return Err(err(
                            format!("{} is a column; name its table.", quote_name(&spec.name)),
                            names(&cols),
                        ));
                    }
                    return Err(err(
                        format!("There's no measure {}.", quote_name(&spec.name)),
                        closest(
                            &spec.name,
                            model
                                .tables
                                .iter()
                                .flat_map(|t| t.measures.iter().map(|m| quote_name(&m.name))),
                        ),
                    ));
                }
                many => {
                    return Err(err(
                        format!(
                            "Several tables have a measure {}; name its table.",
                            quote_name(&spec.name)
                        ),
                        many.iter()
                            .map(|(t, m)| {
                                format!("{}{}", quote_table(&t.name), quote_name(&m.name))
                            })
                            .collect(),
                    ));
                }
            }
        }
    };
    if let (Some(agg), FieldRef::Measure { .. }) = (spec.aggregation, &field) {
        return Err(err(
            format!(
                "{} is a measure: it is aggregated already; drop the {}.",
                quote_name(&spec.name),
                aggregation_name(agg)
            ),
            vec![],
        ));
    }
    Ok(Projection {
        field,
        aggregation: spec.aggregation,
        display_name: None,
    })
}

/// Parses and resolves.
pub fn field(model: &Model, src: &str) -> Result<Projection, NameError> {
    resolve(model, &parse_field(src)?)
}

/// A field's reference, as the agent writes it.
pub fn format_field(model: &Model, f: &FieldRef) -> String {
    match f {
        FieldRef::Column { table, column } => {
            let t = model.table(table);
            let tn = t.map_or(table.as_str(), |t| t.name.as_str());
            let cn = t
                .and_then(|t| t.column(column))
                .map_or(column.as_str(), |c| c.name.as_str());
            format!("{}{}", quote_table(tn), quote_name(cn))
        }
        FieldRef::Measure { table, measure } => {
            let name = model
                .table(table)
                .and_then(|t| t.measures.iter().find(|m| &m.id == measure))
                .map_or(measure.as_str(), |m| m.name.as_str());
            quote_name(name)
        }
    }
}

/// A projection's reference, with its aggregation.
pub fn format_projection(model: &Model, p: &Projection) -> String {
    let f = format_field(model, &p.field);
    let f = match p.aggregation {
        Some(a) => format!("{}({f})", aggregation_name(a)),
        None => f,
    };
    match &p.display_name {
        Some(n) => format!("{f} as \"{n}\""),
        None => f,
    }
}

/// A page by id or name.
pub fn find_page<'a>(model: &'a Model, key: &str) -> Result<&'a Page, NameError> {
    let pages = &model.report.pages;
    pages
        .iter()
        .find(|p| p.id == key)
        .or_else(|| {
            pages
                .iter()
                .find(|p| p.settings.name.eq_ignore_ascii_case(key.trim()))
        })
        .ok_or_else(|| {
            err(
                format!("There's no page \"{key}\"."),
                closest(
                    key,
                    pages.iter().map(|p| format!("\"{}\"", p.settings.name)),
                ),
            )
        })
}

/// A visual by id, or by title (on `page` when given, else anywhere).
pub fn find_visual<'a>(
    model: &'a Model,
    page: Option<&Page>,
    key: &str,
) -> Result<(&'a Page, &'a Visual), NameError> {
    if let Some(found) = model.report.visual(key) {
        return Ok(found);
    }
    let all: Vec<(&Page, &Visual)> = model
        .report
        .pages
        .iter()
        .filter(|p| page.is_none_or(|q| q.id == p.id))
        .flat_map(|p| p.visuals.iter().map(move |v| (p, v)))
        .collect();
    let titled: Vec<_> = all
        .iter()
        .filter(|(_, v)| {
            charted_core::visualq::visual_title(model, v).eq_ignore_ascii_case(key.trim())
        })
        .collect();
    match titled.as_slice() {
        [one] => Ok(**one),
        [] => Err(err(
            format!("There's no visual \"{key}\"; give its id or title."),
            closest(
                key,
                all.iter()
                    .map(|(_, v)| format!("\"{}\"", charted_core::visualq::visual_title(model, v))),
            ),
        )),
        _ => Err(err(
            format!("Several visuals are titled \"{key}\"; give its id."),
            titled.iter().map(|(_, v)| v.id.clone()).collect(),
        )),
    }
}

/// Up to three of `names` nearest to `name` (case-insensitive edit distance).
pub fn closest(name: &str, names: impl Iterator<Item = String>) -> Vec<String> {
    let want = name.to_lowercase();
    let mut scored: Vec<(usize, String)> = names
        .map(|n| {
            let bare = n
                .to_lowercase()
                .trim_matches(|c| c == '\'' || c == '"' || c == '[' || c == ']')
                .to_string();
            let d = if bare.contains(&want) || want.contains(&bare) {
                0
            } else {
                distance(&want, &bare)
            };
            (d, n)
        })
        .collect();
    scored.sort();
    let limit = (want.chars().count() / 2).max(3);
    scored
        .into_iter()
        .filter(|(d, _)| *d <= limit)
        .take(3)
        .map(|(_, n)| n)
        .collect()
}

fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            cur.push(
                (prev[j] + usize::from(ca != cb))
                    .min(prev[j + 1] + 1)
                    .min(cur[j] + 1),
            );
        }
        prev = cur;
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_parse() {
        let f = |t: Option<&str>, n: &str, a| FieldSpec {
            table: t.map(str::to_string),
            name: n.into(),
            aggregation: a,
        };
        assert_eq!(
            parse_field("'Sales'[Amount]").unwrap(),
            f(Some("Sales"), "Amount", None)
        );
        assert_eq!(
            parse_field("Sales[Amount]").unwrap(),
            f(Some("Sales"), "Amount", None)
        );
        assert_eq!(
            parse_field(" [Total Sales] ").unwrap(),
            f(None, "Total Sales", None)
        );
        assert_eq!(
            parse_field("'Bob''s'[a]]b]").unwrap(),
            f(Some("Bob's"), "a]b", None)
        );
        assert_eq!(
            parse_field("sum( 'Sales'[Amount] )").unwrap(),
            f(Some("Sales"), "Amount", Some(Aggregation::Sum))
        );
        assert!(parse_field("Sales").is_err());
        assert!(parse_field("'Sales[Amount]").is_err());
        assert!(parse_field("CALCULATE([x])").is_err());
        assert!(parse_field("[a] + 1").is_err());
    }

    #[test]
    fn near_names_are_suggested() {
        let names = ["'Sales'", "'Store'", "'Date'", "'Product'"].map(String::from);
        assert_eq!(closest("sales", names.clone().into_iter())[0], "'Sales'");
        assert_eq!(
            closest("Prodct", names.into_iter()),
            vec!["'Product'".to_string()]
        );
    }
}
