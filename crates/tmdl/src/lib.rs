//! Writes a charted model as a Power BI project: TMDL for the semantic model (tables,
//! columns, measures, relationships, an M partition per table generated from its load
//! steps) inside a PBIP folder that Power BI Desktop opens. Pure: returns the files.
//!
//! Layout:
//! ```text
//! <Name>.pbip
//! <Name>.SemanticModel/definition.pbism
//! <Name>.SemanticModel/definition/{database,model,relationships,expressions}.tmdl
//! <Name>.SemanticModel/definition/tables/<Table>.tmdl
//! <Name>.Report/definition.pbir
//! <Name>.Report/definition/…            the report as PBIR (see `pbir`)
//! ```

pub mod mquery;
pub mod pbir;
pub mod read;

use charted_compiler::steps::{Query, Source};
use charted_model::*;
use serde_json::json;
use std::fmt::Write;

/// A file to write: a path relative to the export folder, and its text.
pub type File = (String, String);

/// TMDL names are quoted when they contain anything but letters, digits and `_`.
pub fn name(n: &str) -> String {
    if !n.is_empty()
        && n.chars().all(|c| c.is_alphanumeric() || c == '_')
        && !n.starts_with(|c: char| c.is_ascii_digit())
    {
        n.to_string()
    } else {
        format!("'{}'", n.replace('\'', "''"))
    }
}

fn data_type(t: DataType) -> &'static str {
    match t {
        DataType::String => "string",
        DataType::Int64 => "int64",
        DataType::Double => "double",
        DataType::Decimal => "decimal",
        DataType::Boolean => "boolean",
        DataType::Date | DataType::DateTime | DataType::DateTimeTz | DataType::Time => "dateTime",
        DataType::Binary => "binary",
        DataType::Unknown => "string",
    }
}

/// `summarizeBy`, unless it is the default (Power BI's own: numbers sum).
fn summarize(c: &Column) -> Option<&'static str> {
    Some(match c.summarize_by {
        Summarize::None => "none",
        Summarize::Sum => "sum",
        Summarize::Average => "average",
        Summarize::Min => "min",
        Summarize::Max => "max",
        Summarize::Count => "count",
        Summarize::DistinctCount => "distinctCount",
        Summarize::Default => return None,
    })
}

/// A DAX expression after `=`: inline when it is one line, otherwise on the following
/// lines, indented one level deeper than `indent`.
fn expression(out: &mut String, expr: &str, indent: usize) {
    let lines: Vec<&str> = expr.trim().lines().collect();
    if lines.len() <= 1 {
        let _ = writeln!(out, " {}", expr.trim());
        return;
    }
    out.push('\n');
    let pad = "\t".repeat(indent + 2);
    for l in lines {
        let _ = writeln!(out, "{pad}{}", l.trim_end());
    }
}

fn prop(out: &mut String, indent: usize, key: &str, value: impl std::fmt::Display) {
    let _ = writeln!(out, "{}{key}: {value}", "\t".repeat(indent));
}

use charted_compiler::steps::{m_document, m_steps, m_string, m_type};

/// The M query that loads a table, reading files relative to the `SourceFolder`
/// parameter. `queries` are the project's queries (merges and appends name them).
pub fn m_query(q: &Query, table: Option<&Table>, queries: &[Query]) -> String {
    let mut steps = m_steps(q, queries);
    // Text sources need their column types set, unless the query already does it last;
    // typed sources (Parquet, databases) carry them.
    let typed_last = matches!(
        q.steps.last(),
        Some(charted_compiler::steps::Step::ChangeTypes { .. })
    );
    let text = matches!(
        q.source,
        Source::Csv { .. } | Source::Json { .. } | Source::Excel { .. } | Source::Folder { .. }
    );
    if let (Some(t), true, false) = (table, text, typed_last) {
        let types: Vec<String> = t
            .columns
            .iter()
            .filter_map(|c| match &c.kind {
                ColumnKind::Data { source_column } => Some(format!(
                    "{{{}, {}}}",
                    m_string(source_column),
                    m_type(c.data_type)
                )),
                _ => None,
            })
            .collect();
        if !types.is_empty() {
            let prev = charted_compiler::steps::m_ident(&steps.last().unwrap().0);
            let mut name = "Changed Type".to_string();
            let mut k = 2;
            while steps.iter().any(|(x, _)| *x == name) {
                name = format!("Changed Type {k}");
                k += 1;
            }
            steps.push((
                name,
                format!(
                    "Table.TransformColumnTypes({prev}, {{{}}}{})",
                    types.join(", "),
                    // Numbers written "1.234,5" read in a culture that writes them so.
                    if matches!(&q.source, Source::Csv { decimal: Some(d), .. } if d == ",") {
                        ", \"de-DE\""
                    } else {
                        ""
                    }
                ),
            ));
        }
    }
    m_document(&steps)
}

fn table_tmdl(t: &Table, query: Option<&Query>, queries: &[Query]) -> String {
    let mut out = String::new();
    if let Some(d) = &t.description {
        for l in d.lines() {
            let _ = writeln!(out, "/// {l}");
        }
    }
    let _ = writeln!(out, "table {}", name(&t.name));
    prop(&mut out, 1, "lineageTag", &t.id);
    if t.is_date_table {
        prop(&mut out, 1, "dataCategory", "Time");
    }
    if t.hidden {
        let _ = writeln!(out, "\tisHidden");
    }
    for m in &t.measures {
        out.push('\n');
        if let Some(d) = &m.description {
            for l in d.lines() {
                let _ = writeln!(out, "\t/// {l}");
            }
        }
        let _ = write!(out, "\tmeasure {} =", name(&m.name));
        expression(&mut out, &m.expression, 1);
        if let Some(f) = &m.format_string {
            prop(&mut out, 2, "formatString", f);
        }
        if let Some(f) = &m.display_folder {
            prop(&mut out, 2, "displayFolder", f);
        }
        if m.hidden {
            let _ = writeln!(out, "\t\tisHidden");
        }
        prop(&mut out, 2, "lineageTag", &m.id);
    }
    for c in &t.columns {
        out.push('\n');
        if let Some(d) = &c.description {
            for l in d.lines() {
                let _ = writeln!(out, "\t/// {l}");
            }
        }
        match &c.kind {
            ColumnKind::Data { .. } => {
                let _ = writeln!(out, "\tcolumn {}", name(&c.name));
            }
            ColumnKind::Calculated { expression: e, .. } => {
                let _ = write!(out, "\tcolumn {} =", name(&c.name));
                expression(&mut out, e, 1);
            }
        }
        prop(&mut out, 2, "dataType", data_type(c.data_type));
        if c.is_key {
            let _ = writeln!(out, "\t\tisKey");
        }
        if c.hidden {
            let _ = writeln!(out, "\t\tisHidden");
        }
        let default_format = c.format_string.is_none() && c.data_type == DataType::Date;
        match (&c.format_string, c.data_type) {
            (Some(f), _) => prop(&mut out, 2, "formatString", f),
            (None, DataType::Date) => prop(&mut out, 2, "formatString", "Short Date"),
            _ => {}
        }
        if let Some(f) = &c.display_folder {
            prop(&mut out, 2, "displayFolder", f);
        }
        if let Some(cat) = &c.data_category {
            prop(&mut out, 2, "dataCategory", cat);
        }
        prop(&mut out, 2, "lineageTag", &c.id);
        if let Some(s) = summarize(c) {
            prop(&mut out, 2, "summarizeBy", s);
        }
        if let ColumnKind::Data { source_column } = &c.kind {
            prop(&mut out, 2, "sourceColumn", source_column);
        }
        if let Some(by) = &c.sort_by
            && let Some(s) = t.column(by)
        {
            prop(&mut out, 2, "sortByColumn", name(&s.name));
        }
        out.push('\n');
        let _ = writeln!(out, "\t\tannotation SummarizationSetBy = Automatic");
        // Power BI has one date/time type: keep charted's exact one.
        if c.data_type.is_temporal() {
            let _ = writeln!(
                out,
                "\t\tannotation chartedDataType = {}",
                serde_json::to_value(c.data_type)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_default()
            );
        }
        if default_format {
            let _ = writeln!(out, "\t\tannotation chartedDefaultFormat = true");
        }
    }
    for h in &t.hierarchies {
        out.push('\n');
        if let Some(d) = &h.description {
            for l in d.lines() {
                let _ = writeln!(out, "\t/// {l}");
            }
        }
        let _ = writeln!(out, "\thierarchy {}", name(&h.name));
        if h.hidden {
            let _ = writeln!(out, "\t\tisHidden");
        }
        if let Some(f) = &h.display_folder {
            prop(&mut out, 2, "displayFolder", f);
        }
        prop(&mut out, 2, "lineageTag", &h.id);
        for l in &h.levels {
            let Some(c) = t.column(&l.column) else {
                continue;
            };
            out.push('\n');
            let _ = writeln!(out, "\t\tlevel {}", name(&l.name));
            prop(
                &mut out,
                3,
                "lineageTag",
                logical_id(&format!("{}\u{1}{}", h.id, l.name)),
            );
            prop(&mut out, 3, "column", name(&c.name));
        }
    }
    if let Some(q) = query {
        out.push('\n');
        // charted's own query, so importing this file brings it back exactly.
        let _ = writeln!(
            out,
            "\tannotation chartedQuery = {}",
            serde_json::to_string(q).unwrap_or_default()
        );
        out.push('\n');
        let _ = writeln!(out, "\tpartition {} = m", name(&t.name));
        prop(&mut out, 2, "mode", "import");
        let _ = writeln!(out, "\t\tsource =");
        for l in m_query(q, Some(t), queries).lines() {
            let _ = writeln!(out, "\t\t\t\t{l}");
        }
    }
    out
}

fn relationships_tmdl(m: &Model) -> String {
    let mut out = String::new();
    for r in &m.relationships {
        let (Some(ft), Some(tt)) = (m.table(&r.from_table), m.table(&r.to_table)) else {
            continue;
        };
        let (Some(fc), Some(tc)) = (ft.column(&r.from_column), tt.column(&r.to_column)) else {
            continue;
        };
        let _ = writeln!(out, "relationship {}", r.id);
        if !r.active {
            prop(&mut out, 1, "isActive", "false");
        }
        if r.cross_filter == CrossFilter::Both {
            prop(&mut out, 1, "crossFilteringBehavior", "bothDirections");
        }
        if r.from_cardinality == Cardinality::One {
            prop(&mut out, 1, "fromCardinality", "one");
        }
        if r.to_cardinality == Cardinality::Many {
            prop(&mut out, 1, "toCardinality", "many");
        }
        prop(
            &mut out,
            1,
            "fromColumn",
            format!("{}.{}", name(&ft.name), name(&fc.name)),
        );
        prop(
            &mut out,
            1,
            "toColumn",
            format!("{}.{}", name(&tt.name), name(&tc.name)),
        );
        out.push('\n');
    }
    out
}

/// A stable UUID-shaped id for `key` (the same project exports the same ids).
fn logical_id(key: &str) -> String {
    let mut h: [u64; 2] = [0xcbf29ce484222325, 0x84222325cbf29ce4];
    for (i, b) in key.bytes().enumerate() {
        let x = &mut h[i % 2];
        *x ^= u64::from(b);
        *x = x.wrapping_mul(0x100000001b3);
    }
    let hex = format!("{:016x}{:016x}", h[0], h[1]);
    format!(
        "{}-{}-4{}-a{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[13..16],
        &hex[17..20],
        &hex[20..32]
    )
}

/// The files of a PBIP project for `model`, named `project`. `source_folder` is the
/// default for the `SourceFolder` parameter that M queries read files from.
pub fn pbip(project: &str, model: &Model, queries: &[Query], source_folder: &str) -> Vec<File> {
    let sm = format!("{project}.SemanticModel");
    let rp = format!("{project}.Report");
    let def = format!("{sm}/definition");
    let mut files: Vec<File> = Vec::new();

    files.push((
        format!("{project}.pbip"),
        pretty(&json!({
            "$schema": "https://developer.microsoft.com/json-schemas/fabric/pbip/pbipProperties/1.0.0/schema.json",
            "version": "1.0",
            "artifacts": [{ "report": { "path": rp } }],
            "settings": { "enableAutoRecovery": true }
        })),
    ));
    files.push((
        format!("{sm}/definition.pbism"),
        pretty(&json!({
            "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/semanticModel/definitionProperties/1.0.0/schema.json",
            "version": "4.2",
            "settings": {}
        })),
    ));
    files.push((
        format!("{def}/database.tmdl"),
        "database\n\tcompatibilityLevel: 1567\n".into(),
    ));

    let mut model_tmdl = String::from(
        "model Model\n\tculture: en-US\n\tdefaultPowerBIDataSourceVersion: powerBI_V3\n\tsourceQueryCulture: en-US\n\tdataAccessOptions\n\t\tlegacyRedirects\n\t\treturnErrorValuesAsNull\n\n",
    );
    // charted has no automatic date tables; Desktop shouldn't add them on opening.
    model_tmdl.push_str("annotation __PBI_TimeIntelligenceEnabled = 0\n\n");
    let order: Vec<String> = model.tables.iter().map(|t| t.name.clone()).collect();
    let _ = writeln!(
        model_tmdl,
        "annotation PBI_QueryOrder = {}\n",
        serde_json::to_string(&order).unwrap_or_default()
    );
    for t in &model.tables {
        let _ = writeln!(model_tmdl, "ref table {}", name(&t.name));
    }
    model_tmdl.push_str("\nref cultureInfo en-US\n");
    files.push((format!("{def}/model.tmdl"), model_tmdl));
    // The culture Desktop writes into every project (Q&A's linguistic schema).
    files.push((
        format!("{def}/cultures/en-US.tmdl"),
        "cultureInfo en-US\n\n\tlinguisticMetadata =\n\t\t\t{\n\t\t\t  \"Version\": \"1.0.0\",\n\t\t\t  \"Language\": \"en-US\"\n\t\t\t}\n\t\tcontentType: json\n\n".into(),
    ));
    // Fabric's Git integration names each item in a .platform file; Desktop ignores it.
    for (dir, kind) in [(&sm, "SemanticModel"), (&rp, "Report")] {
        files.push((
            format!("{dir}/.platform"),
            pretty(&json!({
                "$schema": "https://developer.microsoft.com/json-schemas/fabric/gitIntegration/platformProperties/2.0.0/schema.json",
                "metadata": { "type": kind, "displayName": project },
                "config": { "version": "2.0", "logicalId": logical_id(&format!("{project}\u{1}{kind}")) }
            })),
        ));
    }

    let folder = if source_folder.ends_with('/') || source_folder.ends_with('\\') {
        source_folder.to_string()
    } else {
        format!("{source_folder}/")
    };
    let mut expressions = format!(
        "/// The folder the data files are read from. Change it where the files live.\nexpression SourceFolder = {} meta [IsParameterQuery=true, Type=\"Text\", IsParameterQueryRequired=true]\n\tlineageTag: {}\n",
        m_string(&folder),
        "6f1d3c2a-8b8e-4f3c-9a51-50757263654f"
    );
    // Queries that only feed merges and appends are shared expressions.
    for q in queries.iter().filter(|q| q.no_load) {
        let _ = writeln!(expressions, "\nexpression {} =", name(&q.name));
        for l in m_query(q, None, queries).lines() {
            let _ = writeln!(expressions, "\t\t{l}");
        }
        let _ = writeln!(expressions, "\tannotation PBI_ResultType = Table");
        let _ = writeln!(
            expressions,
            "\tannotation chartedQuery = {}",
            serde_json::to_string(q).unwrap_or_default()
        );
    }
    files.push((format!("{def}/expressions.tmdl"), expressions));
    if !model.relationships.is_empty() {
        files.push((
            format!("{def}/relationships.tmdl"),
            relationships_tmdl(model),
        ));
    }
    for t in &model.tables {
        let q = queries.iter().find(|q| q.table == t.source);
        files.push((
            format!("{def}/tables/{}.tmdl", file_name(&t.name)),
            table_tmdl(t, q, queries),
        ));
    }

    files.push((
        format!("{rp}/definition.pbir"),
        pretty(&json!({
            "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/report/definitionProperties/2.0.0/schema.json",
            "version": "4.0",
            "datasetReference": { "byPath": { "path": format!("../{sm}") } }
        })),
    ));
    for (path, text) in pbir::report_files(model) {
        files.push((format!("{rp}/{path}"), text));
    }
    files
}

fn pretty(v: &serde_json::Value) -> String {
    let mut s = serde_json::to_string_pretty(v).unwrap_or_default();
    s.push('\n');
    s
}

/// A table name usable as a file name.
fn file_name(n: &str) -> String {
    n.chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect()
}
