//! Imports a Power BI project (a `.pbip` and its semantic model in TMDL, or a
//! `model.bim`) as a new charted project: the model, its queries, the report, and the
//! data from the queries' files. What doesn't come across is listed in the import
//! report.

use charted_compiler::steps::Source;
use charted_engine::{Database, sql};
use charted_model::{ColumnKind, Model};
use charted_tmdl::read::{Imported, Skipped};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use ts_rs::TS;

use crate::project::new_project;
use crate::session::CoreError;

/// What an import brought across, and what it didn't.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImportReport {
    /// The Power BI file imported.
    pub source: String,
    pub tables: usize,
    pub measures: usize,
    pub relationships: usize,
    pub pages: usize,
    pub visuals: usize,
    /// Tables whose data loaded.
    pub loaded: usize,
    pub skipped: Vec<Skipped>,
    /// Things done along the way, e.g. a data file found next to the project.
    pub notes: Vec<String>,
}

fn invalid(m: impl Into<String>) -> CoreError {
    CoreError::Invalid(m.into())
}

/// Every file under `dir`, by path relative to it (text files only).
fn read_tree(dir: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Ok(text) = std::fs::read_to_string(&p)
                && let Ok(rel) = p.strip_prefix(dir)
            {
                out.insert(rel.to_string_lossy().replace('\\', "/"), text);
            }
        }
    }
    out
}

/// The parts of a Power BI project: its name, the semantic model's folder or
/// `model.bim`, and the report folder (if any).
struct Parts {
    name: String,
    model: PathBuf,
    report: Option<PathBuf>,
    /// Where data files are looked for when their paths don't exist here.
    near: PathBuf,
}

fn locate(input: &Path) -> Result<Parts, CoreError> {
    let stem = |p: &Path| {
        p.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default()
    };
    let ext = |p: &Path| {
        p.extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    };
    // A model.bim on its own.
    if input.is_file() && ext(input) == "bim" {
        let dir = input.parent().unwrap_or(Path::new("."));
        let name = dir
            .file_name()
            .map(|n| stem(Path::new(n)))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "Imported".into());
        let name = name.trim_end_matches(".SemanticModel").to_string();
        return Ok(Parts {
            name,
            model: input.to_path_buf(),
            report: None,
            near: dir.to_path_buf(),
        });
    }
    // A .pbip file, or a folder holding one.
    let pbip = if input.is_file() && ext(input) == "pbip" {
        input.to_path_buf()
    } else if input.is_dir() {
        std::fs::read_dir(input)
            .map_err(|e| invalid(format!("couldn't read {}: {e}", input.display())))?
            .flatten()
            .map(|e| e.path())
            .find(|p| ext(p) == "pbip")
            .ok_or_else(|| invalid(format!("{} has no .pbip file", input.display())))?
    } else {
        return Err(invalid(format!(
            "{} isn't a Power BI project (.pbip) or model (.bim)",
            input.display()
        )));
    };
    let dir = pbip.parent().unwrap_or(Path::new(".")).to_path_buf();
    let name = stem(&pbip);
    // The report the .pbip names, and the semantic model that report uses.
    let artifacts: serde_json::Value = std::fs::read_to_string(&pbip)
        .ok()
        .and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())
        .unwrap_or_default();
    let report = artifacts
        .pointer("/artifacts/0/report/path")
        .and_then(|p| p.as_str())
        .map(|p| dir.join(p))
        .unwrap_or_else(|| dir.join(format!("{name}.Report")));
    let pbir: serde_json::Value = std::fs::read_to_string(report.join("definition.pbir"))
        .ok()
        .and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())
        .unwrap_or_default();
    let model = pbir
        .pointer("/datasetReference/byPath/path")
        .and_then(|p| p.as_str())
        .map(|p| report.join(p))
        .unwrap_or_else(|| dir.join(format!("{name}.SemanticModel")));
    if !model.exists() {
        return Err(invalid(format!(
            "the semantic model of {} isn't there ({}); charted can't import a report connected to a published dataset",
            pbip.display(),
            model.display()
        )));
    }
    Ok(Parts {
        name,
        model,
        report: report.exists().then_some(report),
        near: dir,
    })
}

/// The file a source reads, for sources that read one.
fn source_path(s: &mut Source) -> Option<&mut String> {
    match s {
        Source::Csv { path, .. }
        | Source::Parquet { path }
        | Source::Json { path }
        | Source::Excel { path, .. }
        | Source::Folder { path, .. }
        | Source::Sqlite { path, .. } => Some(path),
        Source::Postgres { .. }
        | Source::Calendar { .. }
        | Source::Reference { .. }
        | Source::Table { .. } => None,
    }
}

/// A data file that isn't where the query says: the same file found under `near`,
/// trying the path's endings from the longest (`C:/Users/me/Contoso/data/a.csv` →
/// `near/Contoso/data/a.csv`, `near/data/a.csv`, `near/a.csv`).
fn find_near(path: &str, near: &Path) -> Option<PathBuf> {
    let parts: Vec<&str> = path
        .split(['/', '\\'])
        .filter(|p| !p.is_empty() && !p.ends_with(':'))
        .collect();
    (0..parts.len())
        .map(|i| near.join(parts[i..].join("/")))
        .find(|p| p.exists())
}

/// Imports the Power BI project at `input` as a new charted project in a new folder
/// under `parent`. Returns the project folder and the import report.
pub fn import(input: &Path, parent: &Path) -> Result<(PathBuf, ImportReport), CoreError> {
    let input = &std::fs::canonicalize(input)
        .map_err(|e| invalid(format!("couldn't open {}: {e}", input.display())))?;
    let parts = locate(input)?;
    let Imported {
        mut model,
        mut queries,
        mut skipped,
        parameters,
    } = if parts.model.is_file() {
        let text = std::fs::read_to_string(&parts.model)
            .map_err(|e| invalid(format!("couldn't read {}: {e}", parts.model.display())))?;
        charted_tmdl::read::from_bim(&text, &parts.name).map_err(invalid)?
    } else if parts.model.join("definition").is_dir() {
        charted_tmdl::read::from_tmdl(&read_tree(&parts.model.join("definition")), &parts.name)
    } else if parts.model.join("model.bim").is_file() {
        let text = std::fs::read_to_string(parts.model.join("model.bim"))
            .map_err(|e| invalid(e.to_string()))?;
        charted_tmdl::read::from_bim(&text, &parts.name).map_err(invalid)?
    } else {
        return Err(invalid(format!(
            "{} has neither a definition folder nor a model.bim",
            parts.model.display()
        )));
    };
    if model.tables.is_empty() {
        return Err(invalid(
            "the Power BI model has no tables charted can import",
        ));
    }
    // The report.
    if let Some(r) = &parts.report {
        let report = if r.join("definition/report.json").is_file() {
            charted_tmdl::pbir::read_pbir(&model, &read_tree(r), &mut skipped).map_err(invalid)?
        } else if r.join("report.json").is_file() {
            let text = std::fs::read_to_string(r.join("report.json"))
                .map_err(|e| invalid(e.to_string()))?;
            charted_tmdl::pbir::read_legacy(&model, &text, &mut skipped).map_err(invalid)?
        } else {
            Default::default()
        };
        model.report = report;
    }
    if model.report.pages.is_empty() {
        model.report.pages.push(charted_model::Page {
            id: crate::ids::stable_id(&format!("{}/page", parts.name)),
            settings: charted_model::PageSettings::named("Page 1"),
            visuals: Vec::new(),
        });
    }
    // Files: relative paths of charted's own queries are relative to the folder the
    // export named (`SourceFolder`); anything missing is looked for next to the project.
    let mut notes = Vec::new();
    let folder = parameters.get("SourceFolder").cloned();
    for q in &mut queries {
        let Some(path) = source_path(&mut q.source) else {
            continue;
        };
        if charted_compiler::steps::is_url(path) {
            continue;
        }
        let absolute = path.starts_with('/') || path.chars().nth(1) == Some(':');
        if !absolute && let Some(f) = &folder {
            *path = format!("{}/{}", f.trim_end_matches(['/', '\\']), path);
        }
        if !Path::new(path.as_str()).exists() {
            match find_near(path, &parts.near) {
                Some(found) => {
                    notes.push(format!(
                        "{}: read from {} (the project named {})",
                        q.name,
                        found.display(),
                        path
                    ));
                    *path = found.display().to_string();
                }
                None => skipped.push(Skipped {
                    kind: "data".into(),
                    name: q.name.clone(),
                    reason: format!(
                        "{path} wasn't found; locate it in Transform data, then refresh"
                    ),
                }),
            }
        }
    }
    // A new folder for the project.
    std::fs::create_dir_all(parent)
        .map_err(|e| invalid(format!("couldn't create {}: {e}", parent.display())))?;
    let mut dir = parent.join(&parts.name);
    let mut k = 2;
    while dir.exists() {
        dir = parent.join(format!("{} {k}", parts.name));
        k += 1;
    }
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| parts.name.clone());
    let mut project = new_project(&dir, &name, model, queries);
    // The data.
    let db = Database::open(Some(&project.database_path()), &Default::default())?;
    let dir_str = project.dir_str();
    let sheets = crate::sources::prepare_sheets(&dir_str, &project.cache_dir(), &project.queries)
        .unwrap_or_default();
    let mut loaded = 0;
    let mut have: Vec<String> = Vec::new();
    for q in project.queries.iter().filter(|q| !q.no_load) {
        // Nothing is downloaded without asking: files on the web load on the first
        // Refresh, which asks before getting DuckDB's web reader.
        // A query starting from such a query waits for it too.
        if let Some(web) = q
            .sources_read(&project.queries)
            .into_iter()
            .find(|s| s.is_web())
        {
            skipped.push(Skipped {
                kind: "data".into(),
                name: q.name.clone(),
                reason: format!(
                    "it reads {} from the web; Refresh to load it (charted asks before downloading)",
                    web.path()
                ),
            });
            continue;
        }
        let result = crate::sources::query_sql(&dir_str, &project.queries, &sheets, q, None)
            .and_then(|s| {
                db.materialise(&q.table, &s)
                    .map_err(|e| invalid(e.to_string()))
            });
        match result {
            Ok(_) => {
                loaded += 1;
                have.push(q.table.clone());
            }
            Err(e) if !skipped.iter().any(|s| s.kind == "data" && s.name == q.name) => skipped
                .push(Skipped {
                    kind: "data".into(),
                    name: q.name.clone(),
                    reason: format!("its data didn't load: {e}"),
                }),
            Err(_) => {}
        }
    }
    // Tables without data are created empty, so the model and report still work.
    for t in &project.model.tables {
        if have.contains(&t.source) {
            continue;
        }
        let cols: Vec<String> = t
            .columns
            .iter()
            .filter_map(|c| match &c.kind {
                ColumnKind::Data { source_column } => Some(format!(
                    "{} {}",
                    sql::ident(source_column),
                    charted_compiler::sqltype(c.data_type).unwrap_or("VARCHAR")
                )),
                ColumnKind::Calculated { .. } => None,
            })
            .collect();
        let cols = if cols.is_empty() {
            vec!["\"_\" VARCHAR".to_string()]
        } else {
            cols
        };
        db.execute(&format!(
            "CREATE OR REPLACE TABLE {} ({})",
            sql::ident(&t.source),
            cols.join(", ")
        ))?;
    }
    drop(db);
    // DAX charted can't evaluate yet.
    check_dax(&project.model, &mut skipped);
    project.save()?;
    let m = &project.model;
    let report = ImportReport {
        source: input.display().to_string(),
        tables: m.tables.len(),
        measures: m.tables.iter().map(|t| t.measures.len()).sum(),
        relationships: m.relationships.len(),
        pages: m.report.pages.len(),
        visuals: m.report.pages.iter().map(|p| p.visuals.len()).sum(),
        loaded,
        skipped,
        notes,
    };
    Ok((dir, report))
}

fn check_dax(model: &Model, skipped: &mut Vec<Skipped>) {
    for t in &model.tables {
        for m in &t.measures {
            let (_, diags) = charted_dax::check(model, &m.expression);
            if let Some(d) = diags.first() {
                skipped.push(Skipped {
                    kind: "measure".into(),
                    name: format!("{}[{}]", t.name, m.name),
                    reason: format!("imported, but charted can't evaluate it yet: {}", d.message),
                });
            }
        }
        for c in &t.columns {
            if let ColumnKind::Calculated { expression, .. } = &c.kind {
                let (_, diags) = charted_dax::check_in(model, expression, Some(&t.id));
                if let Some(d) = diags.first() {
                    skipped.push(Skipped {
                        kind: "column".into(),
                        name: format!("{}[{}]", t.name, c.name),
                        reason: format!(
                            "imported, but charted can't compute it yet: {}",
                            d.message
                        ),
                    });
                }
            }
        }
    }
    for r in &model.roles {
        for f in &r.filters {
            if let Some(d) = crate::roles::check_filter(model, &f.table, &f.expression).first() {
                skipped.push(Skipped {
                    kind: "role".into(),
                    name: format!(
                        "{}: {}",
                        r.name,
                        model
                            .table(&f.table)
                            .map(|t| t.name.as_str())
                            .unwrap_or("?")
                    ),
                    reason: format!(
                        "imported, but View as can't apply this filter: {}",
                        d.message
                    ),
                });
            }
        }
    }
}
