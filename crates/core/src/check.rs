//! `charted check`: validates a project without opening it for editing.

use serde::Serialize;
use std::path::Path;

use crate::project::{MODEL_FILE, Project};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckReport {
    pub ok: bool,
    pub project: Option<String>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn check(path: &Path) -> CheckReport {
    let mut report = CheckReport {
        ok: false,
        project: None,
        errors: Vec::new(),
        warnings: Vec::new(),
    };
    let project = match Project::open(path) {
        Ok(p) => p,
        Err(e) => {
            report.errors.push(e.to_string());
            return report;
        }
    };
    report.project = Some(project.manifest.name.clone());
    report.warnings.extend(project.warnings.iter().cloned());

    // The model file must be a valid Apache Ossie document.
    let text = std::fs::read_to_string(project.dir.join(MODEL_FILE)).unwrap_or_default();
    match charted_ossie::yaml_to_json(&text) {
        Ok(doc) => report.errors.extend(
            charted_ossie::validate::validate(&doc)
                .into_iter()
                .map(|e| format!("{MODEL_FILE}: {e}")),
        ),
        Err(e) => report.errors.push(format!("{MODEL_FILE}: {e}")),
    }

    // Every query's source file must exist, and every table must have a query.
    for q in &project.queries {
        let p = charted_compiler::steps::resolve_path(&project.dir_str(), q.source.path());
        if !Path::new(&p).exists() {
            report.errors.push(format!(
                "query \"{}\": source file {} does not exist",
                q.name,
                q.source.path()
            ));
        }
    }
    for t in &project.model.tables {
        if !project.queries.iter().any(|q| q.table == t.source) {
            report.warnings.push(format!(
                "table \"{}\" has no query, so it cannot be refreshed",
                t.name
            ));
        }
    }
    report.ok = report.errors.is_empty();
    report
}
