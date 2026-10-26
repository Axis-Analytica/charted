//! Exports (Power BI project, Apache Ossie) and imports (Ossie), with a readable diff
//! between two models so a round trip can be checked.

use charted_model::{Model, Table};
use serde::{Deserialize, Serialize};
use std::path::Path;
use ts_rs::TS;

use crate::atomic;
use crate::project::{Project, ProjectError};

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> ProjectError + '_ {
    move |source| ProjectError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Measures with a generated OSSIE_SQL_2026 translation next to their DAX.
pub fn generated_dialects(model: &Model) -> charted_ossie::GeneratedDialects {
    let mut generated = charted_ossie::GeneratedDialects::new();
    for m in model.tables.iter().flat_map(|t| &t.measures) {
        if let Some(sql) = charted_compiler::ossie::measure_sql(model, &m.id) {
            generated.insert(
                m.id.clone(),
                vec![(charted_ossie::OSSIE_SQL.to_string(), sql)],
            );
        }
    }
    generated
}

/// Writes a Power BI project (PBIP with TMDL) into `dir`; returns the .pbip path.
pub fn export_pbip(
    project: &Project,
    dir: &Path,
    options: &PbipOptions,
) -> Result<std::path::PathBuf, ProjectError> {
    let name = &project.manifest.name;
    if options.with_data {
        copy_data(project, dir)?;
    }
    let folder = match (&options.source_folder, options.with_data) {
        (Some(f), _) if !f.trim().is_empty() => f.trim().to_string(),
        (_, true) => dir.display().to_string(),
        _ => project.dir_str(),
    };
    let files = charted_tmdl::pbip(name, &project.model, &project.queries, &folder);
    for (rel, text) in &files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io(parent))?;
        }
        atomic::write(&path, text.as_bytes()).map_err(io(&path))?;
    }
    Ok(dir.join(format!("{name}.pbip")))
}

/// How a Power BI project is exported.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PbipOptions {
    /// The folder Power BI reads the data files from (its `SourceFolder` parameter),
    /// such as where they'll be on a Windows computer. By default the export's folder
    /// when the data is copied, else the project's.
    #[serde(default)]
    pub source_folder: Option<String>,
    /// Copies the data files the queries read from the project folder into the
    /// export, so the export is complete on its own.
    #[serde(default)]
    pub with_data: bool,
}

/// Copies the files and folders the queries read from inside the project folder to
/// the same places under `dir`.
fn copy_data(project: &Project, dir: &Path) -> Result<(), ProjectError> {
    for q in &project.queries {
        let path = q.source.path();
        if !q.source.is_file() || path.is_empty() || path.starts_with('/') {
            continue;
        }
        let from = project.dir.join(&path);
        let to = dir.join(&path);
        if from.is_dir() {
            for e in std::fs::read_dir(&from).map_err(io(&from))?.flatten() {
                if e.path().is_file() {
                    let target = to.join(e.file_name());
                    std::fs::create_dir_all(&to).map_err(io(&to))?;
                    std::fs::copy(e.path(), &target).map_err(io(&target))?;
                }
            }
        } else if from.is_file() {
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(io(parent))?;
            }
            std::fs::copy(&from, &to).map_err(io(&to))?;
        }
    }
    Ok(())
}

/// Writes the model as a standalone Apache Ossie document.
pub fn export_ossie(model: &Model, file: &Path) -> Result<(), ProjectError> {
    let text = charted_ossie::write_string(model, &generated_dialects(model));
    atomic::write(file, text.as_bytes()).map_err(io(file))
}

/// Reads an Ossie document as a model, with any import warnings.
pub fn read_ossie(file: &Path) -> Result<(Model, Vec<String>), ProjectError> {
    let text = std::fs::read_to_string(file).map_err(io(file))?;
    let (model, warnings) = charted_ossie::read_str(&text).map_err(|e| ProjectError::Invalid {
        path: file.to_path_buf(),
        message: e.to_string(),
    })?;
    Ok((
        model,
        warnings
            .into_iter()
            .map(|w| format!("{}: {}", w.scope, w.message))
            .collect(),
    ))
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ModelDiff {
    /// One line per difference, e.g. "Sales: measure Margin % added".
    pub differences: Vec<String>,
    pub warnings: Vec<String>,
}

fn json<T: Serialize>(v: &T) -> serde_json::Value {
    serde_json::to_value(v).unwrap_or_default()
}

/// Property names that differ between two serialised objects.
fn changed_props(a: &serde_json::Value, b: &serde_json::Value, skip: &[&str]) -> Vec<String> {
    let (Some(a), Some(b)) = (a.as_object(), b.as_object()) else {
        return if a == b { vec![] } else { vec!["value".into()] };
    };
    let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .filter(|k| !skip.contains(&k.as_str()) && a.get(*k) != b.get(*k))
        .cloned()
        .collect()
}

fn diff_table(out: &mut Vec<String>, a: &Table, b: &Table) {
    let changed = changed_props(&json(a), &json(b), &["columns", "measures"]);
    if !changed.is_empty() {
        out.push(format!("{}: {} changed", a.name, changed.join(", ")));
    }
    for c in &a.columns {
        match b.column(&c.id) {
            None => out.push(format!("{}: column {} removed", a.name, c.name)),
            Some(o) => {
                let ch = changed_props(&json(c), &json(o), &[]);
                if !ch.is_empty() {
                    out.push(format!(
                        "{}: column {} — {} changed",
                        a.name,
                        c.name,
                        ch.join(", ")
                    ));
                }
            }
        }
    }
    for c in &b.columns {
        if a.column(&c.id).is_none() {
            out.push(format!("{}: column {} added", a.name, c.name));
        }
    }
    for m in &a.measures {
        match b.measures.iter().find(|x| x.id == m.id) {
            None => out.push(format!("{}: measure {} removed", a.name, m.name)),
            Some(o) => {
                let ch = changed_props(&json(m), &json(o), &[]);
                if !ch.is_empty() {
                    out.push(format!(
                        "{}: measure {} — {} changed",
                        a.name,
                        m.name,
                        ch.join(", ")
                    ));
                }
            }
        }
    }
    for m in &b.measures {
        if !a.measures.iter().any(|x| x.id == m.id) {
            out.push(format!("{}: measure {} added", a.name, m.name));
        }
    }
}

/// Differences in the semantic model from `a` to `b` (the report is not compared).
pub fn diff(a: &Model, b: &Model) -> Vec<String> {
    let mut out = Vec::new();
    let top = changed_props(&json(a), &json(b), &["tables", "relationships", "report"]);
    if !top.is_empty() {
        out.push(format!("model: {} changed", top.join(", ")));
    }
    for t in &a.tables {
        match b.table(&t.id) {
            None => out.push(format!("table {} removed", t.name)),
            Some(o) => diff_table(&mut out, t, o),
        }
    }
    for t in &b.tables {
        if a.table(&t.id).is_none() {
            out.push(format!("table {} added", t.name));
        }
    }
    let rel_name = |m: &Model, r: &charted_model::Relationship| {
        let col = |t: &str, c: &str| {
            m.table(t)
                .map(|t| {
                    format!(
                        "{}[{}]",
                        t.name,
                        t.column(c).map(|c| c.name.as_str()).unwrap_or("?")
                    )
                })
                .unwrap_or_default()
        };
        format!(
            "{} → {}",
            col(&r.from_table, &r.from_column),
            col(&r.to_table, &r.to_column)
        )
    };
    for r in &a.relationships {
        match b.relationship(&r.id) {
            None => out.push(format!("relationship {} removed", rel_name(a, r))),
            Some(o) => {
                let ch = changed_props(&json(r), &json(o), &[]);
                if !ch.is_empty() {
                    out.push(format!(
                        "relationship {} — {} changed",
                        rel_name(a, r),
                        ch.join(", ")
                    ));
                }
            }
        }
    }
    for r in &b.relationships {
        if a.relationship(&r.id).is_none() {
            out.push(format!("relationship {} added", rel_name(b, r)));
        }
    }
    out
}

/// What importing `file` into a project would change.
pub fn import_diff(current: &Model, file: &Path) -> Result<(Model, ModelDiff), ProjectError> {
    let (mut model, warnings) = read_ossie(file)?;
    // The report is not part of an Ossie document: keep the project's.
    model.report = current.report.clone();
    let differences = diff(current, &model);
    Ok((
        model,
        ModelDiff {
            differences,
            warnings,
        },
    ))
}
