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
//! <Name>.Report/report.json
//! ```

use charted_compiler::steps::{Query, Source, Step};
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

fn summarize(c: &Column) -> &'static str {
    match c.summarize_by {
        Summarize::None => "none",
        Summarize::Sum => "sum",
        Summarize::Average => "average",
        Summarize::Min => "min",
        Summarize::Max => "max",
        Summarize::Count => "count",
        Summarize::DistinctCount => "distinctCount",
        // Power BI's own default: numbers sum, everything else is not summarised.
        Summarize::Default if c.data_type.is_numeric() && !c.is_key => "sum",
        Summarize::Default => "none",
    }
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

/// An M string literal.
fn m_string(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// An M identifier: `#"Changed Type"` when it needs quoting.
fn m_ident(s: &str) -> String {
    if s.chars().all(|c| c.is_alphanumeric() || c == '_') {
        s.to_string()
    } else {
        format!("#{}", m_string(s))
    }
}

fn m_type(t: DataType) -> &'static str {
    match t {
        DataType::String => "type text",
        DataType::Int64 => "Int64.Type",
        DataType::Double => "type number",
        DataType::Decimal => "Currency.Type",
        DataType::Boolean => "type logical",
        DataType::Date => "type date",
        DataType::Time => "type time",
        DataType::DateTime => "type datetime",
        DataType::DateTimeTz => "type datetimezone",
        DataType::Binary | DataType::Unknown => "type any",
    }
}

/// The M query that loads a table, reading files relative to the `SourceFolder`
/// parameter.
pub fn m_query(q: &Query, table: Option<&Table>) -> String {
    let mut steps: Vec<(String, String)> = Vec::new();
    let file = |p: &str| {
        if p.starts_with('/') {
            format!("File.Contents({})", m_string(p))
        } else {
            format!("File.Contents(SourceFolder & {})", m_string(p))
        }
    };
    match &q.source {
        Source::Csv {
            path,
            delimiter,
            header,
            ..
        } => {
            let delim = delimiter.clone().unwrap_or_else(|| ",".into());
            steps.push((
                "Source".into(),
                format!(
                    "Csv.Document({}, [Delimiter={}, Encoding=65001, QuoteStyle=QuoteStyle.Csv])",
                    file(path),
                    m_string(&delim)
                ),
            ));
            if *header {
                steps.push((
                    "Promoted Headers".into(),
                    "Table.PromoteHeaders(Source, [PromoteAllScalars=true])".into(),
                ));
            }
        }
        Source::Parquet { path } => {
            steps.push(("Source".into(), format!("Parquet.Document({})", file(path))))
        }
        Source::Json { path } => {
            steps.push(("Source".into(), format!("Json.Document({})", file(path))));
            steps.push(("To Table".into(), "Table.FromRecords(Source)".into()));
        }
    }
    let prev = |steps: &Vec<(String, String)>| m_ident(&steps.last().unwrap().0);
    for s in &q.steps {
        let list = |xs: &[String]| {
            format!(
                "{{{}}}",
                xs.iter()
                    .map(|x| m_string(x))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let (n, e) = match s {
            Step::SelectColumns { columns } => (
                "Chosen Columns".to_string(),
                format!("Table.SelectColumns({}, {})", prev(&steps), list(columns)),
            ),
            Step::RemoveColumns { columns } => (
                "Removed Columns".to_string(),
                format!("Table.RemoveColumns({}, {})", prev(&steps), list(columns)),
            ),
            Step::RenameColumns { renames } => (
                "Renamed Columns".to_string(),
                format!(
                    "Table.RenameColumns({}, {{{}}})",
                    prev(&steps),
                    renames
                        .iter()
                        .map(|(a, b)| format!("{{{}, {}}}", m_string(a), m_string(b)))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ),
        };
        let mut name = n.clone();
        let mut k = 2;
        while steps.iter().any(|(x, _)| *x == name) {
            name = format!("{n} {k}");
            k += 1;
        }
        steps.push((name, e));
    }
    // Text sources need their column types set; typed sources (Parquet) carry them.
    if let (Some(t), Source::Csv { .. } | Source::Json { .. }) = (table, &q.source) {
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
            steps.push((
                "Changed Type".into(),
                format!(
                    "Table.TransformColumnTypes({}, {{{}}})",
                    prev(&steps),
                    types.join(", ")
                ),
            ));
        }
    }
    let mut out = String::from("let\n");
    for (i, (n, e)) in steps.iter().enumerate() {
        let comma = if i + 1 < steps.len() { "," } else { "" };
        let _ = writeln!(out, "    {} = {e}{comma}", m_ident(n));
    }
    let _ = write!(out, "in\n    {}", m_ident(&steps.last().unwrap().0));
    out
}

fn table_tmdl(t: &Table, query: Option<&Query>) -> String {
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
        prop(&mut out, 2, "summarizeBy", summarize(c));
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
    }
    if let Some(q) = query {
        out.push('\n');
        let _ = writeln!(out, "\tpartition {} = m", name(&t.name));
        prop(&mut out, 2, "mode", "import");
        let _ = writeln!(out, "\t\tsource =");
        for l in m_query(q, Some(t)).lines() {
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
    let order: Vec<String> = model.tables.iter().map(|t| t.name.clone()).collect();
    let _ = writeln!(
        model_tmdl,
        "annotation PBI_QueryOrder = {}\n",
        serde_json::to_string(&order).unwrap_or_default()
    );
    for t in &model.tables {
        let _ = writeln!(model_tmdl, "ref table {}", name(&t.name));
    }
    files.push((format!("{def}/model.tmdl"), model_tmdl));

    let folder = if source_folder.ends_with('/') || source_folder.ends_with('\\') {
        source_folder.to_string()
    } else {
        format!("{source_folder}/")
    };
    files.push((
        format!("{def}/expressions.tmdl"),
        format!(
            "/// The folder the data files are read from. Change it where the files live.\nexpression SourceFolder = {} meta [IsParameterQuery=true, Type=\"Text\", IsParameterQueryRequired=true]\n\tlineageTag: {}\n",
            m_string(&folder),
            "6f1d3c2a-8b8e-4f3c-9a51-50757263654f"
        ),
    ));
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
            table_tmdl(t, q),
        ));
    }

    files.push((
        format!("{rp}/definition.pbir"),
        pretty(&json!({
            "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/report/definitionProperties/1.0.0/schema.json",
            "version": "4.0",
            "datasetReference": { "byPath": { "path": format!("../{sm}") } }
        })),
    ));
    // A report with the pages (visuals follow when report export arrives).
    let sections: Vec<serde_json::Value> = model
        .report
        .pages
        .iter()
        .enumerate()
        .map(|(i, p)| {
            json!({
                "config": "{}",
                "displayName": p.settings.name,
                "displayOption": 1,
                "filters": "[]",
                "height": p.settings.height,
                "name": format!("ReportSection{i}"),
                "ordinal": i,
                "visualContainers": [],
                "width": p.settings.width,
            })
        })
        .collect();
    let sections = if sections.is_empty() {
        vec![
            json!({"config": "{}", "displayName": "Page 1", "displayOption": 1, "filters": "[]", "height": 720, "name": "ReportSection", "visualContainers": [], "width": 1280}),
        ]
    } else {
        sections
    };
    files.push((
        format!("{rp}/report.json"),
        pretty(&json!({
            "config": json!({"version": "5.37", "themeCollection": {}}).to_string(),
            "layoutOptimization": 0,
            "resourcePackages": [],
            "sections": sections,
        })),
    ));
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
