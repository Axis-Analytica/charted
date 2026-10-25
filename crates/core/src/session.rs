//! An open project: model + undo history + loaded data, with crash recovery.

use charted_compiler::steps::{self, Query};
use charted_engine::{sql, Database, DbError, DbOptions};
use charted_model::{Change, ChangeError, History, Model};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use ts_rs::TS;

use crate::atomic;
use crate::project::{Project, ProjectError};

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error(transparent)]
    Project(#[from] ProjectError),
    #[error(transparent)]
    Change(#[from] ChangeError),
    #[error("{0}")]
    Db(#[from] DbError),
    #[error("{0}")]
    Invalid(String),
    #[error("this project is already open in another charted window (process {pid})")]
    Locked { pid: u32 },
    #[error("no project is open")]
    NoProject,
}

impl CoreError {
    /// A stable code the UI can switch on.
    pub fn code(&self) -> &'static str {
        match self {
            CoreError::Project(ProjectError::NotAProject { .. }) => "notAProject",
            CoreError::Project(ProjectError::TooNew { .. }) => "tooNew",
            CoreError::Project(_) => "project",
            CoreError::Change(_) => "invalidChange",
            CoreError::Db(_) => "database",
            CoreError::Invalid(_) => "invalid",
            CoreError::Locked { .. } => "locked",
            CoreError::NoProject => "noProject",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TableStats {
    pub id: String,
    #[ts(type = "number")]
    pub rows: u64,
}

/// How a table is loaded, for display.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QuerySummary {
    pub id: String,
    pub name: String,
    /// The DuckDB table it loads (a model table's `source`).
    pub table: String,
    pub source_path: String,
    pub sql: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RecoveryInfo {
    /// Unix time (seconds) of the unsaved state that can be restored.
    #[ts(type = "number")]
    pub saved_at: u64,
}

/// Everything the UI needs to render the open project.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProjectSnapshot {
    pub name: String,
    pub path: String,
    pub model: Model,
    pub dirty: bool,
    pub undo_label: Option<String>,
    pub redo_label: Option<String>,
    pub tables: Vec<TableStats>,
    pub queries: Vec<QuerySummary>,
    pub warnings: Vec<String>,
    pub recovery: Option<RecoveryInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryFile {
    generation: u64,
    saved_at: u64,
    model: Model,
}

/// Prevents two charted windows editing the same project.
struct Lock {
    path: PathBuf,
}

impl Lock {
    fn acquire(cache: &Path) -> Result<Self, CoreError> {
        let path = cache.join("lock");
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(pid) = text.trim().parse::<u32>() {
                let alive = Path::new(&format!("/proc/{pid}")).exists();
                if alive && pid != std::process::id() {
                    return Err(CoreError::Locked { pid });
                }
            }
        }
        atomic::write(&path, std::process::id().to_string().as_bytes())
            .map_err(|source| ProjectError::Io { path: path.clone(), source })?;
        Ok(Self { path })
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Loads one query's rows into its table.
pub fn load_query(db: &Database, query: &Query, project_dir: &str) -> Result<u64, CoreError> {
    let select = steps::to_sql(query, project_dir);
    db.materialise(&query.table, &select).map_err(|e| {
        CoreError::Invalid(format!("could not load \"{}\" from {}: {e}", query.name, query.source.path()))
    })
}

pub struct Session {
    pub project: Project,
    history: History,
    db: Database,
    recovery: Option<RecoveryInfo>,
    _lock: Lock,
}

impl Session {
    pub fn open(path: &Path) -> Result<Self, CoreError> {
        let project = Project::open(path)?;
        let lock = Lock::acquire(&project.cache_dir())?;
        let db = Database::open(Some(&project.database_path()), &db_options(&project))?;
        for q in &project.queries {
            if !db.table_exists(&q.table)? {
                load_query(&db, q, &project.dir_str())?;
            }
        }
        let recovery = read_recovery(&project).and_then(|r| {
            (r.generation == project.manifest.generation && r.model != project.model).then_some(RecoveryInfo { saved_at: r.saved_at })
        });
        Ok(Self { project, history: History::with_limit(500), db, recovery, _lock: lock })
    }

    pub fn database(&self) -> &Database {
        &self.db
    }

    pub fn snapshot(&self) -> Result<ProjectSnapshot, CoreError> {
        let mut tables = Vec::new();
        for t in &self.project.model.tables {
            let rows = if self.db.table_exists(&t.source)? { self.db.query_u64(&format!("SELECT count(*) FROM {}", sql::ident(&t.source)))? } else { 0 };
            tables.push(TableStats { id: t.id.clone(), rows });
        }
        Ok(ProjectSnapshot {
            name: self.project.manifest.name.clone(),
            path: self.project.manifest_path.display().to_string(),
            model: self.project.model.clone(),
            dirty: self.history.is_dirty(),
            undo_label: self.history.undo_label().map(str::to_string),
            redo_label: self.history.redo_label().map(str::to_string),
            tables,
            queries: self
                .project
                .queries
                .iter()
                .map(|q| QuerySummary {
                    id: q.id.clone(),
                    name: q.name.clone(),
                    table: q.table.clone(),
                    source_path: q.source.path().to_string(),
                    sql: steps::to_sql(q, &self.project.dir_str()),
                })
                .collect(),
            warnings: self.project.warnings.clone(),
            recovery: self.recovery.clone(),
        })
    }

    pub fn commit(&mut self, label: &str, changes: Vec<Change>) -> Result<(), CoreError> {
        self.history.commit(&mut self.project.model, label, changes)?;
        self.write_recovery();
        Ok(())
    }

    pub fn undo(&mut self) -> Result<(), CoreError> {
        self.history.undo(&mut self.project.model)?;
        self.write_recovery();
        Ok(())
    }

    pub fn redo(&mut self) -> Result<(), CoreError> {
        self.history.redo(&mut self.project.model)?;
        self.write_recovery();
        Ok(())
    }

    pub fn is_dirty(&self) -> bool {
        self.history.is_dirty()
    }

    pub fn save(&mut self) -> Result<(), CoreError> {
        self.project.save()?;
        self.history.mark_saved();
        let _ = std::fs::remove_file(recovery_path(&self.project));
        self.recovery = None;
        Ok(())
    }

    /// Replaces the model with the autosaved one. The restore itself is undoable.
    pub fn restore_recovery(&mut self) -> Result<(), CoreError> {
        let r = read_recovery(&self.project).ok_or_else(|| CoreError::Invalid("there is nothing to restore".into()))?;
        let changes = whole_model_changes(&self.project.model, &r.model);
        self.history.commit(&mut self.project.model, "Restore unsaved changes", changes)?;
        self.recovery = None;
        self.write_recovery();
        Ok(())
    }

    pub fn discard_recovery(&mut self) {
        let _ = std::fs::remove_file(recovery_path(&self.project));
        self.recovery = None;
    }

    /// Up to `limit` rows of a model table, as Arrow IPC.
    pub fn preview(&self, table_id: &str, limit: u32) -> Result<Vec<u8>, CoreError> {
        let t = self.project.model.table(table_id).ok_or_else(|| CoreError::Invalid(format!("unknown table {table_id}")))?;
        let source = sql::ident(&t.source);
        Ok(self.db.arrow(&format!("SELECT {} FROM {source} LIMIT {limit}", display_columns(&self.db, &source)?))?)
    }

    /// Runs read-only SQL for the command line and agents.
    pub fn query_arrow(&self, sql_text: &str) -> Result<Vec<u8>, CoreError> {
        Ok(self.db.arrow(sql_text)?)
    }

    fn write_recovery(&mut self) {
        let path = recovery_path(&self.project);
        if !self.history.is_dirty() {
            let _ = std::fs::remove_file(&path);
            return;
        }
        let file = RecoveryFile { generation: self.project.manifest.generation, saved_at: now(), model: self.project.model.clone() };
        if let Ok(json) = serde_json::to_vec(&file) {
            if let Err(e) = atomic::write(&path, &json) {
                tracing::warn!("could not write recovery file {}: {e}", path.display());
            }
        }
    }
}

/// A select list that casts types the webview cannot decode cheaply (DECIMAL, HUGEINT)
/// to DOUBLE. Display only: models and measures keep exact types.
fn display_columns(db: &Database, relation: &str) -> Result<String, CoreError> {
    let cols = db.describe(relation)?;
    Ok(cols
        .iter()
        .map(|c| {
            let id = sql::ident(&c.name);
            let t = c.duckdb_type.to_ascii_uppercase();
            if t.starts_with("DECIMAL") || t == "HUGEINT" || t == "UHUGEINT" {
                format!("CAST({id} AS DOUBLE) AS {id}")
            } else {
                id
            }
        })
        .collect::<Vec<_>>()
        .join(", "))
}

fn db_options(project: &Project) -> DbOptions {
    DbOptions { temp_directory: Some(project.cache_dir().join("spill")), ..Default::default() }
}

fn recovery_path(project: &Project) -> PathBuf {
    project.cache_dir().join("recovery.json")
}

fn read_recovery(project: &Project) -> Option<RecoveryFile> {
    let text = std::fs::read_to_string(recovery_path(project)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Changes that turn `from` into `to` by replacing every table and relationship.
fn whole_model_changes(from: &Model, to: &Model) -> Vec<Change> {
    let mut changes: Vec<Change> = from.relationships.iter().map(|r| Change::RemoveRelationship { relationship: r.id.clone() }).collect();
    changes.extend(from.tables.iter().map(|t| Change::RemoveTable { table: t.id.clone() }));
    changes.extend(to.tables.iter().map(|t| Change::AddTable { table: t.clone(), index: None }));
    changes.extend(to.relationships.iter().map(|r| Change::AddRelationship { relationship: r.clone(), index: None }));
    changes
}
