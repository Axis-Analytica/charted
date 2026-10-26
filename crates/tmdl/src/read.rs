//! Reads a Power BI semantic model — TMDL (a PBIP's `definition/` folder) or TMSL
//! (`model.bim`) — into a charted model and its queries. What charted can't use is
//! listed in the import report instead of being dropped silently:
//! calculation groups, roles, perspectives, translations, calculated tables and M
//! queries it doesn't understand (their tables come in without data).
//!
//! charted's own exports carry each table's query as an annotation (`chartedQuery`),
//! and the exact type of date columns (`chartedDataType`), so they come back exactly.

use charted_compiler::steps::Query;
use charted_model::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;

use crate::mquery;

/// Something in the Power BI model that charted didn't import, or imported partly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Skipped {
    /// What it is: "table", "measure", "hierarchy", "role", "query", "visual"…
    pub kind: String,
    /// Its name, e.g. "Sales[Margin %]".
    pub name: String,
    pub reason: String,
}

/// A semantic model read from Power BI files.
#[derive(Debug, Clone, Default)]
pub struct Imported {
    pub model: Model,
    pub queries: Vec<Query>,
    pub skipped: Vec<Skipped>,
    /// The model's M parameters (such as `SourceFolder`), by name.
    pub parameters: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default)]
struct ImColumn {
    name: String,
    lineage: Option<String>,
    data_type: String,
    source_column: Option<String>,
    expression: Option<String>,
    hidden: bool,
    key: bool,
    format: Option<String>,
    folder: Option<String>,
    category: Option<String>,
    summarize: Option<String>,
    sort_by: Option<String>,
    description: Option<String>,
    annotations: BTreeMap<String, String>,
    row_number: bool,
}

#[derive(Debug, Clone, Default)]
struct ImMeasure {
    name: String,
    lineage: Option<String>,
    expression: String,
    format: Option<String>,
    folder: Option<String>,
    hidden: bool,
    description: Option<String>,
    dynamic_format: bool,
}

#[derive(Debug, Clone, Default)]
struct ImHierarchy {
    name: String,
    lineage: Option<String>,
    description: Option<String>,
    hidden: bool,
    folder: Option<String>,
    /// (ordinal, name, column name)
    levels: Vec<(i64, String, String)>,
}

#[derive(Debug, Clone, Default)]
struct ImPartition {
    kind: String,
    source: String,
    annotations: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default)]
struct ImTable {
    name: String,
    lineage: Option<String>,
    description: Option<String>,
    hidden: bool,
    date_table: bool,
    columns: Vec<ImColumn>,
    measures: Vec<ImMeasure>,
    partitions: Vec<ImPartition>,
    hierarchies: Vec<ImHierarchy>,
    calculation_group: bool,
    annotations: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default)]
struct ImRelationship {
    name: String,
    from: (String, String),
    to: (String, String),
    active: bool,
    both: bool,
    from_many: bool,
    to_many: bool,
}

#[derive(Debug, Clone, Default)]
struct ImExpression {
    name: String,
    expression: String,
    annotations: BTreeMap<String, String>,
}

/// The semantic model, as TMDL and TMSL both describe it.
#[derive(Debug, Clone, Default)]
struct Sm {
    name: String,
    description: Option<String>,
    tables: Vec<ImTable>,
    relationships: Vec<ImRelationship>,
    expressions: Vec<ImExpression>,
    /// Table order (`ref table` in model.tmdl).
    order: Vec<String>,
    /// Model-level objects charted doesn't support: (kind, name).
    other: Vec<(String, String)>,
}

// --- TMDL -------------------------------------------------------------------

/// A line of TMDL: its indentation depth and its text, with the description lines
/// (`///`) before it.
#[derive(Debug, Clone)]
struct Node {
    depth: usize,
    text: String,
    doc: Vec<String>,
    children: Vec<Node>,
}

/// Indentation depth: tabs, or groups of spaces (TMDL can be written with either).
fn depth_of(line: &str, unit: usize) -> usize {
    let mut d = 0;
    let mut spaces = 0;
    for c in line.chars() {
        match c {
            '\t' => d += 1,
            ' ' => {
                spaces += 1;
                if spaces == unit {
                    d += 1;
                    spaces = 0;
                }
            }
            _ => break,
        }
    }
    d
}

fn parse_tree(text: &str) -> Vec<Node> {
    let text = text.trim_start_matches('\u{feff}');
    // The space indent unit, if the file uses spaces.
    let unit = text
        .lines()
        .filter(|l| l.starts_with(' '))
        .map(|l| l.len() - l.trim_start_matches(' ').len())
        .min()
        .unwrap_or(4)
        .max(1);
    let mut flat: Vec<Node> = Vec::new();
    let mut doc = Vec::new();
    let mut fence: Option<(usize, String)> = None;
    for raw in text.lines() {
        // ``` fences hold verbatim expressions.
        if let Some((d, ref mut body)) = fence {
            if raw.trim() == "```" {
                flat.push(Node {
                    depth: d,
                    text: std::mem::take(body),
                    doc: Vec::new(),
                    children: Vec::new(),
                });
                fence = None;
            } else {
                body.push_str(raw.trim_end());
                body.push('\n');
            }
            continue;
        }
        if raw.trim().is_empty() {
            continue;
        }
        let depth = depth_of(raw, unit);
        let t = raw.trim();
        if let Some(d) = t.strip_prefix("///") {
            doc.push(d.trim().to_string());
            continue;
        }
        if t == "```" {
            fence = Some((depth, String::new()));
            continue;
        }
        if let Some(head) = t
            .strip_suffix("```")
            .filter(|h| h.trim_end().ends_with('='))
        {
            // `measure X = ```` starts a fence on the same line.
            flat.push(Node {
                depth,
                text: head.trim_end().to_string(),
                doc: std::mem::take(&mut doc),
                children: Vec::new(),
            });
            fence = Some((depth + 2, String::new()));
            continue;
        }
        flat.push(Node {
            depth,
            text: t.to_string(),
            doc: std::mem::take(&mut doc),
            children: Vec::new(),
        });
    }
    // Nest by depth.
    fn build(flat: &mut std::iter::Peekable<std::vec::IntoIter<Node>>, depth: usize) -> Vec<Node> {
        let mut out: Vec<Node> = Vec::new();
        while let Some(n) = flat.peek() {
            if n.depth < depth {
                break;
            }
            let mut n = flat.next().unwrap();
            n.children = build(flat, n.depth + 1);
            out.push(n);
        }
        out
    }
    let min = flat.iter().map(|n| n.depth).min().unwrap_or(0);
    build(&mut flat.into_iter().peekable(), min)
}

/// A TMDL name at the start of `s`: `'quoted ''name'''` or a bare word; and the rest.
fn take_name(s: &str) -> (String, &str) {
    let s = s.trim_start();
    if let Some(rest) = s.strip_prefix('\'') {
        let mut out = String::new();
        let mut chars = rest.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            if c == '\'' {
                if matches!(chars.peek(), Some((_, '\''))) {
                    chars.next();
                    out.push('\'');
                    continue;
                }
                return (out, &rest[i + 1..]);
            }
            out.push(c);
        }
        return (out, "");
    }
    if let Some(rest) = s.strip_prefix('"') {
        let end = rest.find('"').unwrap_or(rest.len());
        return (rest[..end].to_string(), rest.get(end + 1..).unwrap_or(""));
    }
    let end = s
        .find(|c: char| c.is_whitespace() || c == '=' || c == ':' || c == '.')
        .unwrap_or(s.len());
    (s[..end].to_string(), &s[end..])
}

/// `Table.Column` (either part quoted).
fn dotted(s: &str) -> (String, String) {
    let (a, rest) = take_name(s);
    match rest.trim_start().strip_prefix('.') {
        Some(r) => (a, take_name(r).0),
        None => (String::new(), a),
    }
}

/// An object line: `keyword Name` or `keyword Name = expression`.
fn object(text: &str) -> Option<(String, String, Option<String>)> {
    let (kw, rest) = text.split_once(char::is_whitespace)?;
    if kw.ends_with(':') {
        return None;
    }
    let (name, after) = take_name(rest);
    let after = after.trim_start();
    let expr = after.strip_prefix('=').map(|e| e.trim().to_string());
    Some((kw.to_string(), name, expr))
}

/// A property line: `key: value`, or a flag (`isHidden`).
fn property(text: &str) -> (String, Option<String>) {
    match text.split_once(':') {
        Some((k, v)) if !k.contains(char::is_whitespace) => {
            (k.to_string(), Some(v.trim().to_string()))
        }
        _ => (text.trim().to_string(), None),
    }
}

/// An object's properties (its children that are `key: value` or flags).
fn props(n: &Node) -> BTreeMap<String, String> {
    n.children
        .iter()
        .filter(|c| c.depth == n.depth + 1 && object(&c.text).is_none())
        .map(|c| {
            let (k, v) = property(&c.text);
            (k, v.unwrap_or_else(|| "true".into()))
        })
        .collect()
}

fn annotations(n: &Node) -> BTreeMap<String, String> {
    n.children
        .iter()
        .filter(|c| c.depth == n.depth + 1)
        .filter_map(|c| {
            let (kw, name, expr) = object(&c.text)?;
            (kw == "annotation").then(|| (name, expr.unwrap_or_default()))
        })
        .collect()
}

/// An expression after `=`: inline, or the lines indented under the object (deeper than
/// its properties), keeping their relative indentation.
fn expression_of(n: &Node, inline: Option<String>) -> String {
    if let Some(e) = inline.filter(|e| !e.is_empty()) {
        return e;
    }
    let mut lines = Vec::new();
    fn walk(n: &Node, base: usize, lines: &mut Vec<String>) {
        lines.push(format!(
            "{}{}",
            "\t".repeat(n.depth.saturating_sub(base)),
            n.text
        ));
        for c in &n.children {
            walk(c, base, lines);
        }
    }
    for c in n.children.iter().take_while(|c| c.depth >= n.depth + 2) {
        walk(c, n.depth + 2, &mut lines);
    }
    lines.join("\n")
}

fn doc(n: &Node) -> Option<String> {
    (!n.doc.is_empty()).then(|| n.doc.join("\n"))
}

fn tmdl_table(n: &Node, name: String) -> ImTable {
    let p = props(n);
    let mut t = ImTable {
        name,
        lineage: p.get("lineageTag").cloned(),
        description: doc(n),
        hidden: p.contains_key("isHidden"),
        date_table: p.get("dataCategory").is_some_and(|c| c == "Time"),
        annotations: annotations(n),
        ..Default::default()
    };
    for c in n.children.iter().filter(|c| c.depth == n.depth + 1) {
        if c.text.starts_with("calculationGroup") {
            t.calculation_group = true;
            continue;
        }
        let Some((kw, name, expr)) = object(&c.text) else {
            continue;
        };
        let cp = props(c);
        match kw.as_str() {
            "column" => t.columns.push(ImColumn {
                lineage: cp.get("lineageTag").cloned(),
                data_type: cp
                    .get("dataType")
                    .cloned()
                    .unwrap_or_else(|| "string".into()),
                source_column: cp.get("sourceColumn").cloned(),
                expression: expr.map(|e| expression_of(c, Some(e))),
                hidden: cp.contains_key("isHidden"),
                key: cp.contains_key("isKey"),
                format: cp.get("formatString").cloned(),
                folder: cp.get("displayFolder").cloned(),
                category: cp.get("dataCategory").cloned(),
                summarize: cp.get("summarizeBy").cloned(),
                sort_by: cp.get("sortByColumn").map(|s| take_name(s).0),
                description: doc(c),
                annotations: annotations(c),
                row_number: cp.get("type").is_some_and(|t| t == "rowNumber"),
                name,
            }),
            "measure" => t.measures.push(ImMeasure {
                lineage: cp.get("lineageTag").cloned(),
                expression: expression_of(c, expr),
                format: cp.get("formatString").cloned(),
                folder: cp.get("displayFolder").cloned(),
                hidden: cp.contains_key("isHidden"),
                description: doc(c),
                dynamic_format: c
                    .children
                    .iter()
                    .any(|x| x.text.starts_with("formatStringDefinition")),
                name,
            }),
            "partition" => {
                let kind = expr.unwrap_or_default();
                let source = c
                    .children
                    .iter()
                    .find_map(|s| {
                        let (k, v) = s.text.split_once('=')?;
                        (k.trim() == "source").then(|| expression_of(s, Some(v.trim().to_string())))
                    })
                    .unwrap_or_default();
                t.partitions.push(ImPartition {
                    kind,
                    source,
                    annotations: annotations(c),
                });
            }
            "hierarchy" => {
                let levels = c
                    .children
                    .iter()
                    .filter(|l| l.depth == c.depth + 1)
                    .filter_map(|l| {
                        let (kw, name, _) = object(&l.text)?;
                        (kw == "level").then_some((l, name))
                    })
                    .enumerate()
                    .map(|(i, (l, name))| {
                        let lp = props(l);
                        let ordinal = lp
                            .get("ordinal")
                            .and_then(|o| o.parse().ok())
                            .unwrap_or(i as i64);
                        let column = lp.get("column").map(|c| take_name(c).0).unwrap_or_default();
                        (ordinal, name, column)
                    })
                    .collect();
                t.hierarchies.push(ImHierarchy {
                    lineage: cp.get("lineageTag").cloned(),
                    description: doc(c),
                    hidden: cp.contains_key("isHidden"),
                    folder: cp.get("displayFolder").cloned(),
                    levels,
                    name,
                })
            }
            "calculationGroup" => t.calculation_group = true,
            _ => {}
        }
    }
    t
}

fn tmdl_relationship(n: &Node, name: String) -> ImRelationship {
    let p = props(n);
    ImRelationship {
        name,
        from: dotted(p.get("fromColumn").map(String::as_str).unwrap_or_default()),
        to: dotted(p.get("toColumn").map(String::as_str).unwrap_or_default()),
        active: p.get("isActive").is_none_or(|v| v != "false"),
        both: p
            .get("crossFilteringBehavior")
            .is_some_and(|v| v == "bothDirections"),
        from_many: p.get("fromCardinality").is_none_or(|v| v == "many"),
        to_many: p.get("toCardinality").is_some_and(|v| v == "many"),
    }
}

/// Reads TMDL files (paths relative to the `definition` folder, and their text).
fn read_tmdl(files: &BTreeMap<String, String>) -> Sm {
    let mut sm = Sm::default();
    for (path, text) in files {
        if !path.ends_with(".tmdl") {
            continue;
        }
        for n in parse_tree(text) {
            let Some((kw, name, expr)) = object(&n.text) else {
                continue;
            };
            match kw.as_str() {
                "model" => {
                    sm.description = doc(&n);
                    for c in &n.children {
                        if let Some((k, name, _)) = object(&c.text) {
                            match k.as_str() {
                                "ref" => {
                                    let (what, rest) = c.text["ref".len()..]
                                        .trim()
                                        .split_once(char::is_whitespace)
                                        .unwrap_or(("", ""));
                                    if what == "table" {
                                        sm.order.push(take_name(rest).0);
                                    }
                                }
                                "perspective" | "role" | "culture" => sm.other.push((k, name)),
                                _ => {}
                            }
                        }
                    }
                }
                "table" => sm.tables.push(tmdl_table(&n, name)),
                "relationship" => sm.relationships.push(tmdl_relationship(&n, name)),
                "expression" => {
                    let e = expression_of(&n, expr);
                    sm.expressions.push(ImExpression {
                        name,
                        expression: e,
                        annotations: annotations(&n),
                    });
                }
                // A culture is only lost when it translates names; Desktop writes one
                // with just its linguistic schema into every project.
                "cultureInfo" | "culture"
                    if !n
                        .children
                        .iter()
                        .any(|c| c.text.trim_start().starts_with("translations")) => {}
                "role" | "perspective" | "cultureInfo" | "culture" => {
                    sm.other.push((kw.replace("cultureInfo", "culture"), name))
                }
                _ => {}
            }
        }
    }
    sm
}

// --- TMSL (model.bim) -----------------------------------------------------------------

/// Text that TMSL may store as one string or as an array of lines.
fn lines_or_text(v: Option<&serde_json::Value>) -> String {
    match v {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(a)) => a
            .iter()
            .filter_map(|l| l.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn bim_annotations(v: &serde_json::Value) -> BTreeMap<String, String> {
    v.get("annotations")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    Some((
                        x.get("name")?.as_str()?.to_string(),
                        lines_or_text(x.get("value")),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn read_bim(text: &str) -> Result<Sm, String> {
    let v: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("model.bim isn't valid JSON: {e}"))?;
    let m = v.get("model").unwrap_or(&v);
    let s = |x: &serde_json::Value, k: &str| x.get(k).and_then(|v| v.as_str()).map(str::to_string);
    let b = |x: &serde_json::Value, k: &str| x.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    let arr = |x: &serde_json::Value, k: &str| {
        x.get(k)
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
    };
    let mut sm = Sm {
        name: s(&v, "name").unwrap_or_default(),
        description: s(m, "description"),
        ..Default::default()
    };
    for t in arr(m, "tables") {
        let mut it = ImTable {
            name: s(&t, "name").unwrap_or_default(),
            lineage: s(&t, "lineageTag"),
            description: s(&t, "description"),
            hidden: b(&t, "isHidden"),
            date_table: s(&t, "dataCategory").is_some_and(|c| c == "Time"),
            annotations: bim_annotations(&t),
            calculation_group: t.get("calculationGroup").is_some(),
            hierarchies: arr(&t, "hierarchies")
                .iter()
                .map(|h| ImHierarchy {
                    name: s(h, "name").unwrap_or_default(),
                    lineage: s(h, "lineageTag"),
                    description: s(h, "description"),
                    hidden: b(h, "isHidden"),
                    folder: s(h, "displayFolder"),
                    levels: arr(h, "levels")
                        .iter()
                        .enumerate()
                        .map(|(i, l)| {
                            (
                                l.get("ordinal")
                                    .and_then(|o| o.as_i64())
                                    .unwrap_or(i as i64),
                                s(l, "name").unwrap_or_default(),
                                s(l, "column").unwrap_or_default(),
                            )
                        })
                        .collect(),
                })
                .collect(),
            ..Default::default()
        };
        for c in arr(&t, "columns") {
            let kind = s(&c, "type").unwrap_or_default();
            it.columns.push(ImColumn {
                name: s(&c, "name").unwrap_or_default(),
                lineage: s(&c, "lineageTag"),
                data_type: s(&c, "dataType").unwrap_or_else(|| "string".into()),
                source_column: s(&c, "sourceColumn"),
                expression: (kind == "calculated").then(|| lines_or_text(c.get("expression"))),
                hidden: b(&c, "isHidden"),
                key: b(&c, "isKey"),
                format: s(&c, "formatString"),
                folder: s(&c, "displayFolder"),
                category: s(&c, "dataCategory"),
                summarize: s(&c, "summarizeBy"),
                sort_by: s(&c, "sortByColumn"),
                description: s(&c, "description"),
                annotations: bim_annotations(&c),
                row_number: kind == "rowNumber",
            });
        }
        for me in arr(&t, "measures") {
            it.measures.push(ImMeasure {
                name: s(&me, "name").unwrap_or_default(),
                lineage: s(&me, "lineageTag"),
                expression: lines_or_text(me.get("expression")),
                format: s(&me, "formatString"),
                folder: s(&me, "displayFolder"),
                hidden: b(&me, "isHidden"),
                description: s(&me, "description"),
                dynamic_format: me.get("formatStringDefinition").is_some(),
            });
        }
        for p in arr(&t, "partitions") {
            let src = p.get("source").cloned().unwrap_or_default();
            it.partitions.push(ImPartition {
                kind: s(&src, "type").unwrap_or_else(|| "m".into()),
                source: lines_or_text(src.get("expression").or(src.get("query"))),
                annotations: bim_annotations(&p),
            });
        }
        sm.tables.push(it);
    }
    for r in arr(m, "relationships") {
        sm.relationships.push(ImRelationship {
            name: s(&r, "name").unwrap_or_default(),
            from: (
                s(&r, "fromTable").unwrap_or_default(),
                s(&r, "fromColumn").unwrap_or_default(),
            ),
            to: (
                s(&r, "toTable").unwrap_or_default(),
                s(&r, "toColumn").unwrap_or_default(),
            ),
            active: r.get("isActive").and_then(|v| v.as_bool()).unwrap_or(true),
            both: s(&r, "crossFilteringBehavior").is_some_and(|v| v == "bothDirections"),
            from_many: s(&r, "fromCardinality").is_none_or(|v| v == "many"),
            to_many: s(&r, "toCardinality").is_some_and(|v| v == "many"),
        });
    }
    for e in arr(m, "expressions") {
        sm.expressions.push(ImExpression {
            name: s(&e, "name").unwrap_or_default(),
            expression: lines_or_text(e.get("expression")),
            annotations: bim_annotations(&e),
        });
    }
    for (k, kind) in [
        ("roles", "role"),
        ("perspectives", "perspective"),
        ("cultures", "culture"),
    ] {
        for x in arr(m, k) {
            if kind == "culture" && x.get("translations").is_none() {
                continue;
            }
            sm.other
                .push((kind.into(), s(&x, "name").unwrap_or_default()));
        }
    }
    Ok(sm)
}

// --- Into charted ---------------------------------------------------------------

fn data_type(t: &str, format: Option<&str>) -> DataType {
    match t.to_ascii_lowercase().as_str() {
        "int64" => DataType::Int64,
        "double" => DataType::Double,
        "decimal" => DataType::Decimal,
        "boolean" => DataType::Boolean,
        "binary" => DataType::Binary,
        // Power BI has one date/time type; the format says whether times are shown.
        "datetime" => match format {
            Some(f)
                if f.contains('h') || f.contains('H') || f.eq_ignore_ascii_case("General Date") =>
            {
                DataType::DateTime
            }
            _ => DataType::Date,
        },
        _ => DataType::String,
    }
}

fn summarize(s: Option<&str>) -> Summarize {
    match s.map(str::to_ascii_lowercase).as_deref() {
        Some("none") => Summarize::None,
        Some("sum") => Summarize::Sum,
        Some("average") => Summarize::Average,
        Some("min") => Summarize::Min,
        Some("max") => Summarize::Max,
        Some("count") => Summarize::Count,
        Some("distinctcount") => Summarize::DistinctCount,
        _ => Summarize::Default,
    }
}

/// A DuckDB table name for a model table.
fn source_name(name: &str, taken: &[String]) -> String {
    let base: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    let base = if base.is_empty() {
        "table".to_string()
    } else {
        base
    };
    let mut out = base.clone();
    let mut k = 2;
    while taken.contains(&out) {
        out = format!("{base}_{k}");
        k += 1;
    }
    out
}

/// The parameter values M queries refer to (`SourceFolder = "C:\data\" meta [...]`).
fn parameters(sm: &Sm) -> BTreeMap<String, String> {
    sm.expressions
        .iter()
        .filter(|e| {
            e.expression.contains("IsParameterQuery=true")
                || e.expression.contains("IsParameterQuery = true")
        })
        .filter_map(|e| {
            let v = e.expression.split(" meta ").next()?.trim();
            Some((e.name.clone(), mquery::string_literal(v)?))
        })
        .collect()
}

fn into_charted(sm: Sm, project_name: &str) -> Imported {
    // Objects without a lineage tag get ids made from their names, so importing the
    // same files twice gives the same ids.
    let counter = std::cell::Cell::new(0u64);
    let new_id = || {
        counter.set(counter.get() + 1);
        uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_OID,
            format!("{project_name}/{}", counter.get()).as_bytes(),
        )
        .to_string()
    };
    let mut skipped = Vec::new();
    let mut skip = |kind: &str, name: String, reason: &str| {
        skipped.push(Skipped {
            kind: kind.into(),
            name,
            reason: reason.into(),
        })
    };
    let params = parameters(&sm);
    let mut model = Model {
        name: if sm.name.is_empty() || sm.name == "Model" {
            project_name.to_string()
        } else {
            sm.name.clone()
        },
        description: sm.description.clone(),
        ..Default::default()
    };
    let mut queries: Vec<Query> = Vec::new();
    // Queries read from M, whose references name other queries rather than give ids.
    let mut by_name: Vec<String> = Vec::new();
    let mut taken: Vec<String> = Vec::new();
    let mut tables = sm.tables.clone();
    if !sm.order.is_empty() {
        tables.sort_by_key(|t| {
            sm.order
                .iter()
                .position(|o| *o == t.name)
                .unwrap_or(usize::MAX)
        });
    }
    for t in &tables {
        // Power BI's automatic date tables are hidden helpers: charted has its own.
        if t.name.starts_with("LocalDateTable_") || t.name.starts_with("DateTableTemplate_") {
            skip(
                "table",
                t.name.clone(),
                "an automatic date table (charted uses the model's date table instead)",
            );
            continue;
        }
        if t.calculation_group {
            skip(
                "table",
                t.name.clone(),
                "calculation groups aren't supported yet",
            );
            continue;
        }
        let id = t.lineage.clone().unwrap_or_else(&new_id);
        // The query: charted's own (annotation), or one understood from M.
        let partition = t.partitions.first();
        let own: Option<Query> = partition
            .and_then(|p| p.annotations.get("chartedQuery"))
            .or_else(|| t.annotations.get("chartedQuery"))
            .and_then(|j| serde_json::from_str(j).ok());
        let query = match (own, partition) {
            (Some(q), _) => Some(q),
            (None, Some(p)) if p.kind.eq_ignore_ascii_case("m") => {
                match mquery::to_query(&p.source, &params) {
                    Ok((source, steps)) => {
                        let id = new_id();
                        by_name.push(id.clone());
                        Some(Query {
                            id,
                            name: t.name.clone(),
                            table: source_name(&t.name, &taken),
                            source,
                            steps,
                            step_names: Default::default(),
                            no_load: false,
                        })
                    }
                    Err(why) => {
                        skip(
                            "query",
                            t.name.clone(),
                            &format!(
                                "its Power Query (M) wasn't understood ({why}); the table comes in without data"
                            ),
                        );
                        None
                    }
                }
            }
            (None, Some(p)) if p.kind.eq_ignore_ascii_case("calculated") => {
                skip(
                    "table",
                    t.name.clone(),
                    "calculated tables aren't supported yet; it comes in without data",
                );
                None
            }
            (None, Some(p)) => {
                skip(
                    "query",
                    t.name.clone(),
                    &format!(
                        "its {} partition isn't supported; the table comes in without data",
                        p.kind
                    ),
                );
                None
            }
            (None, None) => None,
        };
        let source = query
            .as_ref()
            .map(|q| q.table.clone())
            .unwrap_or_else(|| source_name(&t.name, &taken));
        taken.push(source.clone());
        if let Some(q) = query {
            queries.push(q);
        }
        let mut columns: Vec<Column> = Vec::new();
        for c in &t.columns {
            if c.row_number {
                continue;
            }
            columns.push(Column {
                id: c.lineage.clone().unwrap_or_else(&new_id),
                name: c.name.clone(),
                data_type: c
                    .annotations
                    .get("chartedDataType")
                    .and_then(|d| serde_json::from_value(serde_json::Value::String(d.clone())).ok())
                    .unwrap_or_else(|| data_type(&c.data_type, c.format.as_deref())),
                kind: match &c.expression {
                    Some(e) => ColumnKind::Calculated {
                        expression: e.clone(),
                        language: ExpressionLanguage::Dax,
                    },
                    None => ColumnKind::Data {
                        source_column: c.source_column.clone().unwrap_or_else(|| c.name.clone()),
                    },
                },
                description: c.description.clone(),
                format_string: c.format.clone().filter(|f| {
                    !(f == "Short Date" && c.annotations.contains_key("chartedDefaultFormat"))
                }),
                summarize_by: summarize(c.summarize.as_deref()),
                data_category: c.category.clone(),
                sort_by: None,
                display_folder: c.folder.clone(),
                hidden: c.hidden,
                is_key: c.key,
                extras: c
                    .annotations
                    .get("chartedGroup")
                    .and_then(|g| serde_json::from_str::<serde_json::Value>(g).ok())
                    .map(|g| [("charted".to_string(), serde_json::json!({ "group": g }))].into())
                    .unwrap_or_default(),
            });
        }
        // Sort-by columns by name, now that every column has an id.
        for c in &t.columns {
            if let Some(by) = &c.sort_by {
                let target = columns.iter().find(|x| x.name == *by).map(|x| x.id.clone());
                if let Some(col) = columns.iter_mut().find(|x| x.name == c.name) {
                    col.sort_by = target;
                }
            }
        }
        let measures = t
            .measures
            .iter()
            .map(|m| {
                if m.dynamic_format {
                    skip("measure", format!("{}[{}]", t.name, m.name), "dynamic format strings aren't supported; the measure keeps its static format");
                }
                Measure {
                    id: m.lineage.clone().unwrap_or_else(&new_id),
                    name: m.name.clone(),
                    expression: m.expression.clone(),
                    language: ExpressionLanguage::Dax,
                    description: m.description.clone(),
                    format_string: m.format.clone(),
                    display_folder: m.folder.clone(),
                    hidden: m.hidden,
                    extras: Default::default(),
                }
            })
            .collect();
        let mut hierarchies: Vec<Hierarchy> = Vec::new();
        for h in &t.hierarchies {
            let mut levels = h.levels.clone();
            levels.sort_by_key(|l| l.0);
            let mut out = Vec::new();
            for (_, level, column) in &levels {
                match columns.iter().find(|c| c.name == *column) {
                    Some(c) if !out.iter().any(|l: &Level| l.column == c.id) => out.push(Level {
                        name: if level.is_empty() {
                            column.clone()
                        } else {
                            level.clone()
                        },
                        column: c.id.clone(),
                    }),
                    _ => skip(
                        "hierarchy level",
                        format!("{}[{}].{}", t.name, h.name, level),
                        "its column wasn't imported",
                    ),
                }
            }
            if out.is_empty() {
                continue;
            }
            hierarchies.push(Hierarchy {
                id: h.lineage.clone().unwrap_or_else(&new_id),
                name: h.name.clone(),
                description: h.description.clone(),
                levels: out,
                hidden: h.hidden,
                display_folder: h.folder.clone(),
                extras: Default::default(),
            });
        }
        model.tables.push(Table {
            id,
            name: t.name.clone(),
            description: t.description.clone(),
            source,
            columns,
            measures,
            hierarchies,
            hidden: t.hidden,
            is_date_table: t.date_table,
            extras: Default::default(),
        });
    }
    // Queries that only feed others (shared expressions that aren't parameters).
    for e in &sm.expressions {
        if params.contains_key(&e.name) {
            continue;
        }
        let own: Option<Query> = e
            .annotations
            .get("chartedQuery")
            .and_then(|j| serde_json::from_str(j).ok());
        match own {
            Some(q) => queries.push(q),
            None => match mquery::to_query(&e.expression, &params) {
                Ok((source, steps)) => {
                    let id = new_id();
                    by_name.push(id.clone());
                    queries.push(Query {
                        id,
                        name: e.name.clone(),
                        table: source_name(&e.name, &taken),
                        source,
                        steps,
                        step_names: Default::default(),
                        no_load: true,
                    })
                }
                Err(why) => skip(
                    "query",
                    e.name.clone(),
                    &format!("this Power Query (M) wasn't understood ({why})"),
                ),
            },
        }
    }
    // Queries read from M refer to others by name: now every query has its id. Those
    // starting from a query that didn't come in don't either.
    let ids: BTreeMap<String, String> = queries
        .iter()
        .map(|q| (q.name.clone(), q.id.clone()))
        .collect();
    let mut gone: Vec<(String, String)> = Vec::new();
    for q in queries.iter_mut() {
        if let charted_compiler::steps::Source::Reference { query } = &mut q.source
            && by_name.contains(&q.id)
        {
            match ids.get(query.as_str()) {
                Some(id) => *query = id.clone(),
                None => gone.push((q.id.clone(), query.clone())),
            }
        }
    }
    // Queries starting from one that didn't come in don't either.
    while let Some((id, other)) = gone.pop() {
        let Some(i) = queries.iter().position(|q| q.id == id) else {
            continue;
        };
        let q = queries.remove(i);
        for o in &queries {
            if matches!(&o.source, charted_compiler::steps::Source::Reference { query } if *query == q.id)
            {
                gone.push((o.id.clone(), q.name.clone()));
            }
        }
        skip(
            "query",
            q.name,
            &format!(
                "it starts from the query \"{other}\", which wasn't imported{}",
                if q.no_load {
                    ""
                } else {
                    "; the table comes in without data"
                }
            ),
        );
    }
    for r in &sm.relationships {
        let find = |(tn, cn): &(String, String)| {
            let t = model.tables.iter().find(|t| t.name == *tn)?;
            let c = t.columns.iter().find(|c| c.name == *cn)?;
            Some((t.id.clone(), c.id.clone()))
        };
        match (find(&r.from), find(&r.to)) {
            (Some((ft, fc)), Some((tt, tc))) => model.relationships.push(Relationship {
                id: if r.name.is_empty() {
                    new_id()
                } else {
                    r.name.clone()
                },
                from_table: ft,
                from_column: fc,
                to_table: tt,
                to_column: tc,
                from_cardinality: if r.from_many {
                    Cardinality::Many
                } else {
                    Cardinality::One
                },
                to_cardinality: if r.to_many {
                    Cardinality::Many
                } else {
                    Cardinality::One
                },
                cross_filter: if r.both {
                    CrossFilter::Both
                } else {
                    CrossFilter::Single
                },
                active: r.active,
                extras: Default::default(),
            }),
            _ => {
                // Relationships to automatic date tables go with them.
                if !(r.to.0.starts_with("LocalDateTable_")
                    || r.to.0.starts_with("DateTableTemplate_"))
                {
                    skip(
                        "relationship",
                        format!("{}[{}] → {}[{}]", r.from.0, r.from.1, r.to.0, r.to.1),
                        "one of its columns wasn't imported",
                    );
                }
            }
        }
    }
    for (kind, name) in &sm.other {
        let reason = match kind.as_str() {
            "role" => "row-level security roles aren't supported",
            "perspective" => "perspectives aren't supported",
            _ => "translations aren't supported",
        };
        skip(kind, name.clone(), reason);
    }
    Imported {
        model,
        queries,
        skipped,
        parameters: params,
    }
}

/// Reads a semantic model from a PBIP's `definition/` TMDL files (paths relative to
/// that folder).
pub fn from_tmdl(files: &BTreeMap<String, String>, project_name: &str) -> Imported {
    into_charted(read_tmdl(files), project_name)
}

/// Reads a semantic model from `model.bim` (TMSL JSON).
pub fn from_bim(text: &str, project_name: &str) -> Result<Imported, String> {
    Ok(into_charted(read_bim(text)?, project_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_trees() {
        assert_eq!(take_name("'Sales ''24'[x]"), ("Sales '24".into(), "[x]"));
        assert_eq!(
            dotted("'Date Table'.'Order Date'"),
            ("Date Table".into(), "Order Date".into())
        );
        assert_eq!(dotted("Sales.Amount"), ("Sales".into(), "Amount".into()));
        let t = parse_tree(
            "table Sales\n\tlineageTag: abc\n\n\tmeasure 'Total' =\n\t\t\tSUM ( Sales[Amount] )\n\t\t\t+ 1\n\t\tformatString: 0\n\n\tcolumn Amount\n\t\tdataType: double\n",
        );
        assert_eq!(t.len(), 1);
        let table = tmdl_table(&t[0], "Sales".into());
        assert_eq!(table.lineage.as_deref(), Some("abc"));
        assert_eq!(table.measures[0].expression, "SUM ( Sales[Amount] )\n+ 1");
        assert_eq!(table.measures[0].format.as_deref(), Some("0"));
        assert_eq!(table.columns[0].data_type, "double");
    }

    #[test]
    fn hierarchies_from_model_bim() {
        let bim = r#"{"name": "M", "model": {"tables": [{"name": "Product",
            "columns": [{"name": "Category", "dataType": "string", "sourceColumn": "Category"},
                        {"name": "Sub", "dataType": "string", "sourceColumn": "Sub"}],
            "hierarchies": [{"name": "Cats", "lineageTag": "h1", "displayFolder": "F", "isHidden": true,
                "levels": [{"name": "Subcategory", "ordinal": 1, "column": "Sub"},
                           {"name": "Category", "ordinal": 0, "column": "Category"},
                           {"name": "Gone", "ordinal": 2, "column": "Missing"}]}]}]}}"#;
        let im = from_bim(bim, "M").unwrap();
        let t = &im.model.tables[0];
        let h = &t.hierarchies[0];
        assert_eq!((h.id.as_str(), h.name.as_str()), ("h1", "Cats"));
        assert!(h.hidden);
        assert_eq!(h.display_folder.as_deref(), Some("F"));
        let names: Vec<&str> = h.levels.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Category", "Subcategory"]);
        assert_eq!(h.levels[1].column, t.column_by_name("Sub").unwrap().id);
        assert_eq!(im.skipped.len(), 1, "{:?}", im.skipped);
        assert_eq!(im.skipped[0].kind, "hierarchy level");
    }
}
