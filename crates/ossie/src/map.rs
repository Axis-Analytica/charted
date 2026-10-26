//! Mapping between charted's semantic model and Apache Ossie documents.
//!
//! Conventions (see docs/file-format.md):
//! - Tables are datasets; `source` is the DuckDB table that holds the loaded rows.
//! - Data columns are fields with a bare `ANSI_SQL` identifier (quoted when needed);
//!   calculated columns carry their `DAX` expression.
//! - Measures are written as root-level metrics with their home table recorded in the
//!   `POWER_BI` blob, which is where the official Power BI converter looks for it.
//! - Power BI properties (format strings, hidden flags, cardinality, cross-filtering…)
//!   live in `POWER_BI` blobs using TMSL property names.
//! - Anything charted does not interpret is kept in each object's `extras` and written
//!   back unchanged.

use charted_model::*;
use serde_json::{Map, Value, json};

use crate::document::{self as doc, Document, Expression, SPEC_VERSION};
use crate::stash::{
    self, CHARTED, CHARTED_STASH_VERSION, POWER_BI, POWER_BI_STASH_VERSION, StashError,
};

pub const DAX: &str = "DAX";
pub const ANSI_SQL: &str = "ANSI_SQL";
pub const OSSIE_SQL: &str = "OSSIE_SQL_2026";

/// Expressions generated from the authored DAX, keyed by object id, written next to it
/// so that tools without DAX support can evaluate them.
pub type GeneratedDialects = std::collections::BTreeMap<Id, Vec<(String, String)>>;

#[derive(Debug, thiserror::Error)]
pub enum MapError {
    #[error(transparent)]
    Stash(#[from] StashError),
    #[error("unsupported Apache Ossie version \"{0}\" (charted reads {SPEC_VERSION})")]
    Version(String),
    #[error("{0}")]
    Invalid(String),
}

/// Something that was imported with reduced fidelity.
#[derive(Debug, Clone, PartialEq)]
pub struct Warning {
    pub scope: String,
    pub message: String,
}

// Extras keys.
const X_DIALECTS: &str = "ossie.dialects";
const X_EXTENSIONS: &str = "ossie.customExtensions";
const X_AI_CONTEXT: &str = "ossie.aiContext";
const X_LABEL: &str = "ossie.label";
const X_UNKNOWN: &str = "ossie.unknown";
const X_POWERBI: &str = "powerbi";
const X_UNIQUE_KEYS: &str = "ossie.uniqueKeys";
const X_IS_TIME: &str = "ossie.isTime";
const X_CHARTED: &str = "charted";

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// Builds the Ossie document for `model`.
pub fn to_document(model: &Model, generated: &GeneratedDialects) -> Document {
    let mut d = Document {
        version: SPEC_VERSION.into(),
        name: model.name.clone(),
        description: model.description.clone(),
        ai_context: model.extras.get(X_AI_CONTEXT).cloned(),
        datasets: model
            .tables
            .iter()
            .map(|t| write_table(model, t, generated))
            .collect(),
        relationships: model
            .relationships
            .iter()
            .filter_map(|r| write_relationship(model, r))
            .collect(),
        metrics: model
            .tables
            .iter()
            .flat_map(|t| {
                t.measures
                    .iter()
                    .map(move |m| write_measure(t, m, generated))
            })
            .collect(),
        custom_extensions: Vec::new(),
        unknown: unknown_of(&model.extras),
    };
    finish_extensions(
        &mut d.custom_extensions,
        &model.extras,
        Map::new(),
        Map::new(),
    );
    d
}

fn unknown_of(extras: &Extras) -> doc::Unknown {
    extras
        .get(X_UNKNOWN)
        .and_then(|v| v.as_object())
        .map(|m| m.clone().into_iter().collect())
        .unwrap_or_default()
}

/// Writes POWER_BI and CHARTED blobs (merging preserved unknown keys) and foreign extensions.
fn finish_extensions(
    exts: &mut Vec<doc::CustomExtension>,
    extras: &Extras,
    mut powerbi: Map<String, Value>,
    mut charted: Map<String, Value>,
) {
    if let Some(Value::Object(preserved)) = extras.get(X_POWERBI) {
        for (k, v) in preserved {
            powerbi.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    if let Some(Value::Object(preserved)) = extras.get(X_CHARTED) {
        for (k, v) in preserved {
            charted.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    if let Some(Value::Array(foreign)) = extras.get(X_EXTENSIONS) {
        for e in foreign {
            if let Ok(e) = serde_json::from_value::<doc::CustomExtension>(e.clone()) {
                exts.push(e);
            }
        }
    }
    stash::write(exts, POWER_BI, POWER_BI_STASH_VERSION, powerbi);
    stash::write(exts, CHARTED, CHARTED_STASH_VERSION, charted);
}

fn expression(
    primary: (&str, &str),
    id: &str,
    extras: &Extras,
    generated: &GeneratedDialects,
) -> Expression {
    let mut e = Expression::single(primary.0, primary.1);
    for (dialect, text) in generated.get(id).into_iter().flatten() {
        if e.get(dialect).is_none() {
            e.dialects.push(doc::DialectExpression {
                dialect: dialect.clone(),
                expression: text.clone(),
            });
        }
    }
    if let Some(Value::Array(others)) = extras.get(X_DIALECTS) {
        for o in others {
            if let Ok(o) = serde_json::from_value::<doc::DialectExpression>(o.clone())
                && e.get(&o.dialect).is_none()
            {
                e.dialects.push(o);
            }
        }
    }
    e
}

/// Records which dialects were generated, so readers regenerate rather than preserve them.
fn mark_generated(charted: &mut Map<String, Value>, id: &str, generated: &GeneratedDialects) {
    if let Some(list) = generated.get(id).filter(|l| !l.is_empty()) {
        charted.insert(
            "generated".into(),
            Value::Array(list.iter().map(|(d, _)| Value::from(d.clone())).collect()),
        );
    }
}

fn is_plain_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The ANSI SQL expression for a physical column: bare when possible (the converter maps
/// a bare identifier straight to a TMSL `sourceColumn`), double-quoted otherwise.
fn column_reference(name: &str) -> String {
    if is_plain_identifier(name) {
        name.to_string()
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

fn datatype(t: DataType) -> Option<&'static str> {
    Some(match t {
        DataType::String => "String",
        DataType::Int64 => "Integer",
        DataType::Double => "Float",
        DataType::Decimal => "Decimal",
        DataType::Boolean => "Boolean",
        DataType::Date => "Date",
        DataType::Time => "Time",
        DataType::DateTime => "DateTime",
        DataType::DateTimeTz => "DateTimeTz",
        DataType::Binary => "Opaque",
        DataType::Unknown => return None,
    })
}

fn summarize_tmsl(s: Summarize) -> Option<&'static str> {
    Some(match s {
        Summarize::Default => return None,
        Summarize::None => "none",
        Summarize::Sum => "sum",
        Summarize::Average => "average",
        Summarize::Min => "min",
        Summarize::Max => "max",
        Summarize::Count => "count",
        Summarize::DistinctCount => "distinctCount",
    })
}

fn put<T: Into<Value>>(m: &mut Map<String, Value>, k: &str, v: Option<T>) {
    if let Some(v) = v {
        m.insert(k.into(), v.into());
    }
}

fn write_table(model: &Model, t: &Table, generated: &GeneratedDialects) -> doc::Dataset {
    let mut pb = Map::new();
    pb.insert("lineageTag".into(), t.id.clone().into());
    if t.hidden {
        pb.insert("isHidden".into(), true.into());
    }
    if t.is_date_table {
        pb.insert("dataCategory".into(), "Time".into());
    }
    let mut d = doc::Dataset {
        name: t.name.clone(),
        source: t.source.clone(),
        primary_key: t
            .columns
            .iter()
            .filter(|c| c.is_key)
            .map(|c| c.name.clone())
            .collect(),
        unique_keys: t
            .extras
            .get(X_UNIQUE_KEYS)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default(),
        description: t.description.clone(),
        ai_context: t.extras.get(X_AI_CONTEXT).cloned(),
        fields: t
            .columns
            .iter()
            .map(|c| write_column(t, c, generated))
            .collect(),
        metrics: Vec::new(),
        custom_extensions: Vec::new(),
        unknown: unknown_of(&t.extras),
    };
    let _ = model;
    if !t.hierarchies.is_empty() {
        pb.insert(
            "hierarchies".into(),
            Value::Array(
                t.hierarchies
                    .iter()
                    .map(|h| write_hierarchy(t, h))
                    .collect(),
            ),
        );
    }
    finish_extensions(&mut d.custom_extensions, &t.extras, pb, Map::new());
    d
}

/// A hierarchy as TMSL describes it, which is how the converter keeps it in the table's
/// `POWER_BI` blob (levels name their column; lineage tags are derived for levels).
fn write_hierarchy(t: &Table, h: &Hierarchy) -> Value {
    let mut o = match h.extras.get(X_POWERBI) {
        Some(Value::Object(preserved)) => preserved.clone(),
        _ => Map::new(),
    };
    o.insert("name".into(), h.name.clone().into());
    o.insert("lineageTag".into(), h.id.clone().into());
    put(&mut o, "description", h.description.clone());
    put(&mut o, "displayFolder", h.display_folder.clone());
    if h.hidden {
        o.insert("isHidden".into(), true.into());
    }
    let levels: Vec<Value> = h
        .levels
        .iter()
        .enumerate()
        .map(|(i, l)| {
            json!({
                "name": l.name,
                "ordinal": i,
                "column": t.column(&l.column).map(|c| c.name.as_str()).unwrap_or_default(),
                "lineageTag": derived_id("level", &format!("{}/{}", h.id, l.name)),
            })
        })
        .collect();
    o.insert("levels".into(), Value::Array(levels));
    Value::Object(o)
}

/// Reads the hierarchies in a table's `POWER_BI` blob, resolving level columns by name.
fn read_hierarchies(r: &mut Reader, table: &mut Table, pb: &mut Map<String, Value>) {
    let Some(Value::Array(items)) = pb.remove("hierarchies") else {
        return;
    };
    for item in items {
        let Value::Object(mut o) = item else { continue };
        let name = take_str(&mut o, "name").unwrap_or_default();
        let scope = format!("hierarchy {}[{name}]", table.name);
        let mut levels: Vec<(i64, Level)> = Vec::new();
        let mut missing = false;
        if let Some(Value::Array(ls)) = o.remove("levels") {
            for (i, l) in ls.iter().enumerate() {
                let col = l.get("column").and_then(Value::as_str).unwrap_or_default();
                let Some(c) = table.column_by_name(col) else {
                    missing = true;
                    continue;
                };
                let ordinal = l.get("ordinal").and_then(Value::as_i64).unwrap_or(i as i64);
                let level_name = l.get("name").and_then(Value::as_str).unwrap_or(col);
                levels.push((
                    ordinal,
                    Level {
                        name: level_name.to_string(),
                        column: c.id.clone(),
                    },
                ));
            }
        }
        if missing {
            r.warn(
                &scope,
                "a level refers to a column that does not exist and was skipped",
            );
        }
        if levels.is_empty() {
            r.warn(&scope, "it has no levels and was skipped");
            continue;
        }
        levels.sort_by_key(|(o, _)| *o);
        let mut h = Hierarchy {
            id: take_str(&mut o, "lineageTag")
                .unwrap_or_else(|| derived_id("hierarchy", &format!("{}.{name}", table.name))),
            name,
            description: take_str(&mut o, "description"),
            levels: levels.into_iter().map(|(_, l)| l).collect(),
            hidden: take_bool(&mut o, "isHidden").unwrap_or(false),
            display_folder: take_str(&mut o, "displayFolder"),
            extras: Extras::new(),
        };
        if !o.is_empty() {
            h.extras.insert(X_POWERBI.into(), Value::Object(o));
        }
        table.hierarchies.push(h);
    }
}

fn write_column(t: &Table, c: &Column, generated: &GeneratedDialects) -> doc::Field {
    let mut pb = Map::new();
    let mut ch = Map::new();
    pb.insert("lineageTag".into(), c.id.clone().into());
    put(&mut pb, "formatString", c.format_string.clone());
    put(&mut pb, "summarizeBy", summarize_tmsl(c.summarize_by));
    put(&mut pb, "dataCategory", c.data_category.clone());
    put(&mut pb, "displayFolder", c.display_folder.clone());
    if let Some(sort) = c.sort_by.as_ref().and_then(|id| t.column(id)) {
        pb.insert("sortByColumn".into(), sort.name.clone().into());
    }
    if c.hidden {
        pb.insert("isHidden".into(), true.into());
    }
    if c.is_key {
        pb.insert("isKey".into(), true.into());
    }
    let expr = match &c.kind {
        ColumnKind::Data { source_column } => {
            if source_column != &c.name {
                // The converter maps a bare identifier to `sourceColumn`; record the real
                // source when it differs from the column's display name.
                pb.insert("sourceColumn".into(), source_column.clone().into());
            }
            expression(
                (ANSI_SQL, &column_reference(source_column)),
                &c.id,
                &c.extras,
                generated,
            )
        }
        ColumnKind::Calculated {
            expression: e,
            language,
        } => match language {
            ExpressionLanguage::Dax => expression((DAX, e), &c.id, &c.extras, generated),
            ExpressionLanguage::Sql => {
                ch.insert("expressionLanguage".into(), "sql".into());
                expression((ANSI_SQL, e), &c.id, &c.extras, generated)
            }
        },
    };
    mark_generated(&mut ch, &c.id, generated);
    let is_time = c.extras.get(X_IS_TIME).and_then(Value::as_bool);
    let mut f = doc::Field {
        name: c.name.clone(),
        expression: expr,
        dimension: is_time.map(|v| doc::Dimension {
            is_time: Some(v),
            unknown: Default::default(),
        }),
        label: c
            .extras
            .get(X_LABEL)
            .and_then(|v| v.as_str())
            .map(str::to_string),
        description: c.description.clone(),
        datatype: datatype(c.data_type).map(str::to_string),
        ai_context: c.extras.get(X_AI_CONTEXT).cloned(),
        custom_extensions: Vec::new(),
        unknown: unknown_of(&c.extras),
    };
    finish_extensions(&mut f.custom_extensions, &c.extras, pb, ch);
    f
}

fn write_measure(t: &Table, m: &Measure, generated: &GeneratedDialects) -> doc::Metric {
    let mut pb = Map::new();
    let mut ch = Map::new();
    pb.insert("table".into(), t.name.clone().into());
    pb.insert("lineageTag".into(), m.id.clone().into());
    put(&mut pb, "formatString", m.format_string.clone());
    put(&mut pb, "displayFolder", m.display_folder.clone());
    if m.hidden {
        pb.insert("isHidden".into(), true.into());
    }
    let expr = match m.language {
        ExpressionLanguage::Dax => expression((DAX, &m.expression), &m.id, &m.extras, generated),
        ExpressionLanguage::Sql => {
            ch.insert("expressionLanguage".into(), "sql".into());
            expression((ANSI_SQL, &m.expression), &m.id, &m.extras, generated)
        }
    };
    mark_generated(&mut ch, &m.id, generated);
    let mut out = doc::Metric {
        name: m.name.clone(),
        expression: expr,
        description: m.description.clone(),
        datatype: m
            .extras
            .get("ossie.datatype")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        ai_context: m.extras.get(X_AI_CONTEXT).cloned(),
        custom_extensions: Vec::new(),
        unknown: unknown_of(&m.extras),
    };
    finish_extensions(&mut out.custom_extensions, &m.extras, pb, ch);
    out
}

fn write_relationship(model: &Model, r: &Relationship) -> Option<doc::Relationship> {
    let from = model.table(&r.from_table)?;
    let to = model.table(&r.to_table)?;
    let from_col = from.column(&r.from_column)?;
    let to_col = to.column(&r.to_column)?;
    let mut pb = Map::new();
    if r.from_cardinality != Cardinality::Many {
        pb.insert("fromCardinality".into(), "one".into());
    }
    if r.to_cardinality != Cardinality::One {
        pb.insert("toCardinality".into(), "many".into());
    }
    if r.cross_filter == CrossFilter::Both {
        pb.insert("crossFilteringBehavior".into(), "bothDirections".into());
    }
    if !r.active {
        pb.insert("isActive".into(), false.into());
    }
    let mut ch = Map::new();
    ch.insert("id".into(), r.id.clone().into());
    if !pb.is_empty() {
        // Lets the converter detect stale metadata if endpoints are edited by hand.
        pb.insert(
            "normalizedEndpoints".into(),
            json!([from.name, from_col.name, to.name, to_col.name]),
        );
    }
    let mut out = doc::Relationship {
        name: format!(
            "{}_{}_to_{}_{}",
            from.name, from_col.name, to.name, to_col.name
        ),
        from: from.name.clone(),
        to: to.name.clone(),
        from_columns: vec![from_col.name.clone()],
        to_columns: vec![to_col.name.clone()],
        ai_context: r.extras.get(X_AI_CONTEXT).cloned(),
        custom_extensions: Vec::new(),
        unknown: unknown_of(&r.extras),
    };
    finish_extensions(&mut out.custom_extensions, &r.extras, pb, ch);
    Some(out)
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// Deterministic, GUID-shaped id for objects that arrive without a lineage tag.
fn derived_id(kind: &str, path: &str) -> String {
    // Two independent FNV-1a 64-bit hashes give 128 bits.
    let fnv = |seed: u64| {
        let mut h = seed;
        for b in kind.bytes().chain([0]).chain(path.bytes()) {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    };
    let a = fnv(0xcbf29ce484222325);
    let b = fnv(0x84222325cbf29ce4);
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        a >> 32,
        (a >> 16) & 0xffff,
        a & 0xffff,
        b >> 48,
        b & 0xffff_ffff_ffff
    )
}

struct Reader {
    warnings: Vec<Warning>,
}

impl Reader {
    fn warn(&mut self, scope: impl Into<String>, message: impl Into<String>) {
        self.warnings.push(Warning {
            scope: scope.into(),
            message: message.into(),
        });
    }
}

fn take_str(m: &mut Map<String, Value>, k: &str) -> Option<String> {
    match m.remove(k) {
        Some(Value::String(s)) => Some(s),
        Some(other) => {
            m.insert(k.into(), other);
            None
        }
        None => None,
    }
}

fn take_bool(m: &mut Map<String, Value>, k: &str) -> Option<bool> {
    match m.remove(k) {
        Some(Value::Bool(b)) => Some(b),
        Some(other) => {
            m.insert(k.into(), other);
            None
        }
        None => None,
    }
}

/// Common extras: other dialects, ai_context, foreign extensions, unknown keys, leftover stash keys.
fn collect_extras(
    extras: &mut Extras,
    exts: &[doc::CustomExtension],
    ai_context: Option<&Value>,
    unknown: &doc::Unknown,
    other_dialects: Vec<&doc::DialectExpression>,
    powerbi_left: Map<String, Value>,
    charted_left: Map<String, Value>,
) {
    let foreign = stash::foreign(exts);
    if !foreign.is_empty() {
        extras.insert(X_EXTENSIONS.into(), serde_json::to_value(foreign).unwrap());
    }
    if let Some(ai) = ai_context {
        extras.insert(X_AI_CONTEXT.into(), ai.clone());
    }
    if !unknown.is_empty() {
        extras.insert(
            X_UNKNOWN.into(),
            Value::Object(unknown.clone().into_iter().collect()),
        );
    }
    if !other_dialects.is_empty() {
        extras.insert(
            X_DIALECTS.into(),
            serde_json::to_value(other_dialects).unwrap(),
        );
    }
    if !powerbi_left.is_empty() {
        extras.insert(X_POWERBI.into(), Value::Object(powerbi_left));
    }
    if !charted_left.is_empty() {
        extras.insert(X_CHARTED.into(), Value::Object(charted_left));
    }
}

fn parse_datatype(s: Option<&str>) -> DataType {
    match s {
        Some("String") => DataType::String,
        Some("Integer") => DataType::Int64,
        Some("Float") => DataType::Double,
        Some("Decimal") => DataType::Decimal,
        Some("Boolean") => DataType::Boolean,
        Some("Date") => DataType::Date,
        Some("Time") => DataType::Time,
        Some("DateTime") => DataType::DateTime,
        Some("DateTimeTz") => DataType::DateTimeTz,
        Some("Opaque") => DataType::Binary,
        _ => DataType::Unknown,
    }
}

fn parse_summarize(s: Option<String>) -> Summarize {
    match s.as_deref() {
        Some("none") => Summarize::None,
        Some("sum") => Summarize::Sum,
        Some("average") => Summarize::Average,
        Some("min") => Summarize::Min,
        Some("max") => Summarize::Max,
        Some("count") => Summarize::Count,
        Some("distinctCount") => Summarize::DistinctCount,
        _ => Summarize::Default,
    }
}

/// Parses an ANSI SQL column reference back to a column name, if it is one.
fn parse_column_reference(e: &str) -> Option<String> {
    let e = e.trim();
    if is_plain_identifier(e) {
        return Some(e.to_string());
    }
    let inner = e.strip_prefix('"')?.strip_suffix('"')?;
    if inner.replace("\"\"", "").contains('"') {
        return None;
    }
    Some(inner.replace("\"\"", "\""))
}

/// Picks the primary expression (DAX first, then SQL) and returns the rest as extras.
fn split_expression<'a>(
    e: &'a Expression,
    generated_keys: &[String],
) -> (Option<(&'a str, &'a str)>, Vec<&'a doc::DialectExpression>) {
    let primary = e
        .dialects
        .iter()
        .find(|d| d.dialect == DAX)
        .or_else(|| {
            e.dialects
                .iter()
                .find(|d| d.dialect == ANSI_SQL || d.dialect == OSSIE_SQL)
        })
        .or_else(|| e.dialects.first());
    let others = e
        .dialects
        .iter()
        .filter(|d| Some(d.dialect.as_str()) != primary.map(|p| p.dialect.as_str()))
        // Dialects charted generated are regenerated on save, not preserved.
        .filter(|d| !generated_keys.contains(&d.dialect))
        .collect();
    (
        primary.map(|p| (p.dialect.as_str(), p.expression.as_str())),
        others,
    )
}

fn generated_dialects(charted: &mut Map<String, Value>) -> Vec<String> {
    match charted.remove("generated") {
        Some(Value::Array(items)) => items
            .into_iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

/// Reads an Ossie document into a model, with warnings for anything imported with
/// reduced fidelity.
pub fn from_document(d: &Document) -> Result<(Model, Vec<Warning>), MapError> {
    if d.version != SPEC_VERSION {
        return Err(MapError::Version(d.version.clone()));
    }
    let mut r = Reader {
        warnings: Vec::new(),
    };
    let mut model = Model {
        name: d.name.clone(),
        description: d.description.clone(),
        ..Default::default()
    };

    for ds in &d.datasets {
        let mut pb = stash::read(&ds.custom_extensions, POWER_BI, POWER_BI_STASH_VERSION)?;
        let ch = stash::read(&ds.custom_extensions, CHARTED, CHARTED_STASH_VERSION)?;
        let id = take_str(&mut pb, "lineageTag").unwrap_or_else(|| derived_id("table", &ds.name));
        let hidden = take_bool(&mut pb, "isHidden").unwrap_or(false);
        let is_date_table = match take_str(&mut pb, "dataCategory") {
            Some(c) if c == "Time" => true,
            Some(other) => {
                pb.insert("dataCategory".into(), other.into());
                false
            }
            None => false,
        };
        let mut table = Table {
            id,
            name: ds.name.clone(),
            description: ds.description.clone(),
            source: ds.source.clone(),
            columns: Vec::new(),
            measures: Vec::new(),
            hierarchies: Vec::new(),
            hidden,
            is_date_table,
            extras: Extras::new(),
        };
        let mut sort_by_names: Vec<(usize, String)> = Vec::new();
        for f in &ds.fields {
            let scope = format!("field {}.{}", ds.name, f.name);
            let mut fpb = stash::read(&f.custom_extensions, POWER_BI, POWER_BI_STASH_VERSION)?;
            let mut fch = stash::read(&f.custom_extensions, CHARTED, CHARTED_STASH_VERSION)?;
            let generated = generated_dialects(&mut fch);
            let (primary, others) = split_expression(&f.expression, &generated);
            let sql_language = fch.remove("expressionLanguage").is_some_and(|v| v == "sql");
            let source_override = take_str(&mut fpb, "sourceColumn");
            let kind = match primary {
                Some((DAX, e)) => ColumnKind::Calculated {
                    expression: e.to_string(),
                    language: ExpressionLanguage::Dax,
                },
                Some((dialect, e)) if dialect == ANSI_SQL && !sql_language => {
                    match parse_column_reference(e) {
                        Some(col) => ColumnKind::Data {
                            source_column: source_override.clone().unwrap_or(col),
                        },
                        None => ColumnKind::Calculated {
                            expression: e.to_string(),
                            language: ExpressionLanguage::Sql,
                        },
                    }
                }
                Some((dialect, e)) => {
                    if dialect != ANSI_SQL && dialect != OSSIE_SQL {
                        r.warn(
                            &scope,
                            format!("expression is in {dialect}, which charted cannot evaluate"),
                        );
                    }
                    ColumnKind::Calculated {
                        expression: e.to_string(),
                        language: ExpressionLanguage::Sql,
                    }
                }
                None => {
                    r.warn(
                        &scope,
                        "field has no expression; treated as a column of the same name",
                    );
                    ColumnKind::Data {
                        source_column: f.name.clone(),
                    }
                }
            };
            if let Some(name) = take_str(&mut fpb, "sortByColumn") {
                sort_by_names.push((table.columns.len(), name));
            }
            let mut column = Column {
                id: take_str(&mut fpb, "lineageTag")
                    .unwrap_or_else(|| derived_id("column", &format!("{}.{}", ds.name, f.name))),
                name: f.name.clone(),
                data_type: parse_datatype(f.datatype.as_deref()),
                kind,
                description: f.description.clone(),
                format_string: take_str(&mut fpb, "formatString"),
                summarize_by: parse_summarize(take_str(&mut fpb, "summarizeBy")),
                data_category: take_str(&mut fpb, "dataCategory"),
                sort_by: None,
                display_folder: take_str(&mut fpb, "displayFolder"),
                hidden: take_bool(&mut fpb, "isHidden").unwrap_or(false),
                is_key: take_bool(&mut fpb, "isKey").unwrap_or(false)
                    || ds.primary_key.len() == 1 && ds.primary_key[0] == f.name,
                extras: Extras::new(),
            };
            if let Some(t) = f.dimension.as_ref().and_then(|d| d.is_time) {
                column.extras.insert(X_IS_TIME.into(), t.into());
            }
            if let Some(label) = &f.label {
                column.extras.insert(X_LABEL.into(), label.clone().into());
            }
            collect_extras(
                &mut column.extras,
                &f.custom_extensions,
                f.ai_context.as_ref(),
                &f.unknown,
                others,
                fpb,
                fch,
            );
            table.columns.push(column);
        }
        for (i, name) in sort_by_names {
            match table.column_by_name(&name).map(|c| c.id.clone()) {
                Some(id) => table.columns[i].sort_by = Some(id),
                None => r.warn(
                    format!("table {}", ds.name),
                    format!("sort-by column \"{name}\" does not exist"),
                ),
            }
        }
        read_hierarchies(&mut r, &mut table, &mut pb);
        if ds.primary_key.len() > 1 {
            r.warn(
                format!("table {}", ds.name),
                "composite primary keys are kept but not used by charted",
            );
            table
                .extras
                .insert("ossie.primaryKey".into(), json!(ds.primary_key));
        }
        if !ds.unique_keys.is_empty() {
            table
                .extras
                .insert(X_UNIQUE_KEYS.into(), json!(ds.unique_keys));
        }
        collect_extras(
            &mut table.extras,
            &ds.custom_extensions,
            ds.ai_context.as_ref(),
            &ds.unknown,
            Vec::new(),
            pb,
            ch,
        );
        for m in &ds.metrics {
            let measure = read_metric(&mut r, m, Some(&ds.name))?;
            table.measures.push(measure.1);
        }
        model.tables.push(table);
    }

    for m in &d.metrics {
        let (home, measure) = read_metric(&mut r, m, None)?;
        let index = home
            .as_deref()
            .and_then(|h| {
                model
                    .tables
                    .iter()
                    .position(|t| t.name.eq_ignore_ascii_case(h))
            })
            .unwrap_or_else(|| {
                r.warn(
                    format!("metric {}", m.name),
                    "no home table recorded; placed on the first table",
                );
                0
            });
        match model.tables.get_mut(index) {
            Some(t) => t.measures.push(measure),
            None => r.warn(
                format!("metric {}", m.name),
                "the model has no tables to hold this measure",
            ),
        }
    }

    for rel in &d.relationships {
        let scope = format!("relationship {}", rel.name);
        if rel.from_columns.len() != 1 || rel.to_columns.len() != 1 {
            r.warn(
                &scope,
                "composite relationships are not supported yet and were skipped",
            );
            continue;
        }
        let ends = model
            .table_by_name(&rel.from)
            .zip(model.table_by_name(&rel.to))
            .and_then(|(f, t)| {
                Some((
                    f.id.clone(),
                    f.column_by_name(&rel.from_columns[0])?.id.clone(),
                    t.id.clone(),
                    t.column_by_name(&rel.to_columns[0])?.id.clone(),
                ))
            });
        let Some((from_table, from_column, to_table, to_column)) = ends else {
            r.warn(
                &scope,
                "it refers to tables or columns that do not exist; skipped",
            );
            continue;
        };
        let mut pb = stash::read(&rel.custom_extensions, POWER_BI, POWER_BI_STASH_VERSION)?;
        let mut ch = stash::read(&rel.custom_extensions, CHARTED, CHARTED_STASH_VERSION)?;
        let current_ends = json!([rel.from, rel.from_columns[0], rel.to, rel.to_columns[0]]);
        let stale = pb
            .get("normalizedEndpoints")
            .is_some_and(|e| e != &current_ends);
        pb.remove("normalizedEndpoints");
        pb.remove("name");
        if stale {
            r.warn(
                &scope,
                "its Power BI metadata describes different columns and was ignored",
            );
            for k in [
                "fromCardinality",
                "toCardinality",
                "crossFilteringBehavior",
                "isActive",
                "relyOnReferentialIntegrity",
            ] {
                pb.remove(k);
            }
        }
        let card = |v: Option<String>, default: Cardinality| match v.as_deref() {
            Some("one") => Cardinality::One,
            Some("many") => Cardinality::Many,
            _ => default,
        };
        let relationship = Relationship {
            id: take_str(&mut ch, "id").unwrap_or_else(|| derived_id("relationship", &rel.name)),
            from_table,
            from_column,
            to_table,
            to_column,
            from_cardinality: card(take_str(&mut pb, "fromCardinality"), Cardinality::Many),
            to_cardinality: card(take_str(&mut pb, "toCardinality"), Cardinality::One),
            cross_filter: match take_str(&mut pb, "crossFilteringBehavior").as_deref() {
                Some("bothDirections") => CrossFilter::Both,
                _ => CrossFilter::Single,
            },
            active: take_bool(&mut pb, "isActive").unwrap_or(true),
            extras: Extras::new(),
        };
        let mut rel_out = relationship;
        collect_extras(
            &mut rel_out.extras,
            &rel.custom_extensions,
            rel.ai_context.as_ref(),
            &rel.unknown,
            Vec::new(),
            pb,
            ch,
        );
        model.relationships.push(rel_out);
    }

    let pb = stash::read(&d.custom_extensions, POWER_BI, POWER_BI_STASH_VERSION)?;
    let ch = stash::read(&d.custom_extensions, CHARTED, CHARTED_STASH_VERSION)?;
    collect_extras(
        &mut model.extras,
        &d.custom_extensions,
        d.ai_context.as_ref(),
        &d.unknown,
        Vec::new(),
        pb,
        ch,
    );
    Ok((model, r.warnings))
}

fn read_metric(
    r: &mut Reader,
    m: &doc::Metric,
    dataset: Option<&str>,
) -> Result<(Option<String>, Measure), MapError> {
    let mut pb = stash::read(&m.custom_extensions, POWER_BI, POWER_BI_STASH_VERSION)?;
    let mut ch = stash::read(&m.custom_extensions, CHARTED, CHARTED_STASH_VERSION)?;
    let generated = generated_dialects(&mut ch);
    let home = take_str(&mut pb, "table").or_else(|| dataset.map(str::to_string));
    let name = take_str(&mut pb, "name").unwrap_or_else(|| m.name.clone());
    let (primary, others) = split_expression(&m.expression, &generated);
    let sql = ch.remove("expressionLanguage").is_some_and(|v| v == "sql");
    let (expression, language) = match primary {
        Some((DAX, e)) if !sql => (e.to_string(), ExpressionLanguage::Dax),
        Some((dialect, e)) => {
            if dialect != ANSI_SQL && dialect != OSSIE_SQL && dialect != DAX {
                r.warn(
                    format!("metric {}", m.name),
                    format!("expression is in {dialect}, which charted cannot evaluate"),
                );
            }
            (e.to_string(), ExpressionLanguage::Sql)
        }
        None => {
            return Err(MapError::Invalid(format!(
                "metric {} has no expression",
                m.name
            )));
        }
    };
    let mut measure = Measure {
        id: take_str(&mut pb, "lineageTag").unwrap_or_else(|| {
            derived_id(
                "measure",
                &format!("{}.{}", home.clone().unwrap_or_default(), name),
            )
        }),
        name,
        expression,
        language,
        description: m.description.clone(),
        format_string: take_str(&mut pb, "formatString"),
        display_folder: take_str(&mut pb, "displayFolder"),
        hidden: take_bool(&mut pb, "isHidden").unwrap_or(false),
        extras: Extras::new(),
    };
    if let Some(dt) = &m.datatype {
        measure
            .extras
            .insert("ossie.datatype".into(), dt.clone().into());
    }
    collect_extras(
        &mut measure.extras,
        &m.custom_extensions,
        m.ai_context.as_ref(),
        &m.unknown,
        others,
        pb,
        ch,
    );
    Ok((home, measure))
}
