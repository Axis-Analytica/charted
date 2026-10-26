//! The on-disk project: a git-friendly folder.
//!
//! ```text
//! Sales/
//!   Sales.charted          manifest (JSON); the file you open
//!   model.ossie.yaml       semantic model (Apache Ossie)
//!   queries/<table>.json   how each table is loaded
//!   report/                pages and visuals (see reportfile.rs)
//!   data/                  source files (sample projects only)
//!   .gitignore             ignores .cache/
//!   .cache/data.duckdb     loaded rows (rebuildable), recovery file, lock
//! ```

use charted_compiler::steps::Query;
use charted_model::Model;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::atomic;

pub const MANIFEST_EXT: &str = "charted";
/// Loaded data, recovery and the lock: rebuilt from the project, never shared.
pub const CACHE_DIR: &str = ".cache";
pub const MODEL_FILE: &str = "model.ossie.yaml";
pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path} is not a charted project (no .charted manifest found)")]
    NotAProject { path: PathBuf },
    #[error("{path} contains more than one .charted manifest; open one of them directly")]
    AmbiguousProject { path: PathBuf },
    #[error("{path}: {message}")]
    Invalid { path: PathBuf, message: String },
    #[error(
        "this project was saved by a newer charted (format {found}); update charted to open it"
    )]
    TooNew { found: u32 },
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> ProjectError + '_ {
    move |source| ProjectError::Io {
        path: path.to_path_buf(),
        source,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format: u32,
    pub name: String,
    /// Increases on every save; the recovery file records which save it extends.
    pub generation: u64,
    pub app_version: String,
}

#[derive(Debug, Clone)]
pub struct Project {
    pub dir: PathBuf,
    pub manifest_path: PathBuf,
    pub manifest: Manifest,
    pub model: Model,
    pub queries: Vec<Query>,
    /// Problems found while reading the model, shown to the user.
    pub warnings: Vec<String>,
}

/// Finds the manifest for a folder or manifest path.
pub fn locate(path: &Path) -> Result<PathBuf, ProjectError> {
    if path.is_file() {
        return if path.extension().is_some_and(|e| e == MANIFEST_EXT) {
            Ok(path.to_path_buf())
        } else {
            Err(ProjectError::NotAProject {
                path: path.to_path_buf(),
            })
        };
    }
    let mut found = Vec::new();
    for entry in std::fs::read_dir(path).map_err(io(path))? {
        let p = entry.map_err(io(path))?.path();
        if p.is_file() && p.extension().is_some_and(|e| e == MANIFEST_EXT) {
            found.push(p);
        }
    }
    match found.len() {
        0 => Err(ProjectError::NotAProject {
            path: path.to_path_buf(),
        }),
        1 => Ok(found.remove(0)),
        _ => Err(ProjectError::AmbiguousProject {
            path: path.to_path_buf(),
        }),
    }
}

impl Project {
    pub fn open(path: &Path) -> Result<Self, ProjectError> {
        let manifest_path = locate(path)?;
        let dir = manifest_path
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf();
        let text = std::fs::read_to_string(&manifest_path).map_err(io(&manifest_path))?;
        let manifest: Manifest =
            serde_json::from_str(&text).map_err(|e| ProjectError::Invalid {
                path: manifest_path.clone(),
                message: e.to_string(),
            })?;
        if manifest.format > FORMAT_VERSION {
            return Err(ProjectError::TooNew {
                found: manifest.format,
            });
        }
        let model_path = dir.join(MODEL_FILE);
        let model_text = std::fs::read_to_string(&model_path).map_err(io(&model_path))?;
        let (model, warnings) =
            charted_ossie::read_str(&model_text).map_err(|e| ProjectError::Invalid {
                path: model_path.clone(),
                message: e.to_string(),
            })?;
        let warnings = warnings
            .into_iter()
            .map(|w| format!("{}: {}", w.scope, w.message))
            .collect();
        let queries = read_queries(&dir)?;
        let mut model = model;
        model.report = crate::reportfile::read(&dir)?;
        Ok(Self {
            dir,
            manifest_path,
            manifest,
            model,
            queries,
            warnings,
        })
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.dir.join(CACHE_DIR)
    }

    pub fn database_path(&self) -> PathBuf {
        self.cache_dir().join("data.duckdb")
    }

    pub fn dir_str(&self) -> String {
        self.dir.display().to_string()
    }

    /// Writes every project file atomically, the manifest last, and bumps the generation.
    pub fn save(&mut self) -> Result<(), ProjectError> {
        // Each measure also gets SQL that tools without DAX can evaluate.
        let mut generated = charted_ossie::GeneratedDialects::new();
        for m in self.model.tables.iter().flat_map(|t| &t.measures) {
            if let Some(sql) = charted_compiler::ossie::measure_sql(&self.model, &m.id) {
                generated.insert(
                    m.id.clone(),
                    vec![(charted_ossie::OSSIE_SQL.to_string(), sql)],
                );
            }
        }
        let model_path = self.dir.join(MODEL_FILE);
        atomic::write(
            &model_path,
            charted_ossie::write_string(&self.model, &generated).as_bytes(),
        )
        .map_err(io(&model_path))?;
        let qdir = self.dir.join("queries");
        let mut keep = Vec::new();
        for q in &self.queries {
            let p = qdir.join(format!("{}.json", q.table));
            let mut json = serde_json::to_string_pretty(q).expect("queries serialise");
            json.push('\n');
            atomic::write(&p, json.as_bytes()).map_err(io(&p))?;
            keep.push(p);
        }
        // Remove query files for queries that no longer exist.
        if let Ok(entries) = std::fs::read_dir(&qdir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "json") && !keep.contains(&p) {
                    let _ = std::fs::remove_file(&p);
                }
            }
        }
        crate::reportfile::write(&self.dir, &self.model.report)?;
        let gi = self.dir.join(".gitignore");
        if !gi.exists() {
            atomic::write(&gi, b"# Rebuildable caches written by charted\n.cache/\n")
                .map_err(io(&gi))?;
        }
        self.manifest.generation += 1;
        self.manifest.app_version = env!("CARGO_PKG_VERSION").to_string();
        let mut json = serde_json::to_string_pretty(&self.manifest).expect("manifest serialises");
        json.push('\n');
        atomic::write(&self.manifest_path, json.as_bytes()).map_err(io(&self.manifest_path))?;
        Ok(())
    }
}

fn read_queries(dir: &Path) -> Result<Vec<Query>, ProjectError> {
    let qdir = dir.join("queries");
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&qdir) else {
        return Ok(out);
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();
    for p in paths {
        let text = std::fs::read_to_string(&p).map_err(io(&p))?;
        let q: Query = serde_json::from_str(&text).map_err(|e| ProjectError::Invalid {
            path: p.clone(),
            message: e.to_string(),
        })?;
        out.push(q);
    }
    Ok(out)
}

/// Creates an empty project called `name` in a new folder `<parent>/<name>`, with one
/// blank page, and returns the folder.
pub fn create_empty(parent: &Path, name: &str) -> Result<PathBuf, crate::session::CoreError> {
    use crate::session::CoreError;
    let name = name.trim();
    if name.is_empty() {
        return Err(CoreError::Invalid("the project needs a name".into()));
    }
    if name.contains(['/', '\\']) || name.starts_with('.') || name.chars().any(char::is_control) {
        return Err(CoreError::Invalid(format!(
            "\"{name}\" can't be a folder name; leave out / and \\, and don't start with a dot"
        )));
    }
    let dir = parent.join(name);
    if dir.exists() {
        return Err(CoreError::Invalid(format!(
            "there's already a folder called \"{name}\" in {}; choose another name",
            parent.display()
        )));
    }
    std::fs::create_dir_all(&dir)
        .map_err(|e| CoreError::Invalid(format!("couldn't create {}: {e}", dir.display())))?;
    let model = Model {
        name: name.to_string(),
        report: charted_model::Report {
            pages: vec![charted_model::Page {
                id: uuid::Uuid::new_v4().to_string(),
                settings: charted_model::PageSettings::named("Page 1"),
                visuals: vec![],
            }],
            ..Default::default()
        },
        ..Default::default()
    };
    let mut project = new_project(&dir, name, model, Vec::new());
    project.save()?;
    Ok(dir)
}

/// A new, empty project in `dir`.
pub fn new_project(dir: &Path, name: &str, model: Model, queries: Vec<Query>) -> Project {
    let file = format!("{}.{MANIFEST_EXT}", sanitize_file_name(name));
    Project {
        dir: dir.to_path_buf(),
        manifest_path: dir.join(file),
        manifest: Manifest {
            format: FORMAT_VERSION,
            name: name.to_string(),
            generation: 0,
            app_version: env!("CARGO_PKG_VERSION").into(),
        },
        model,
        queries,
        warnings: Vec::new(),
    }
}

fn sanitize_file_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c == '/' || c == '\0' || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let s = s.trim().trim_start_matches('.').to_string();
    if s.is_empty() { "project".into() } else { s }
}
