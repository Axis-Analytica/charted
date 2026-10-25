//! An open project: model + undo history + loaded data, with crash recovery.

use charted_compiler::steps::{self, Query};
use charted_engine::{Database, DbError, DbOptions, sql};
use charted_model::{Change, ChangeError, ColumnKind, ColumnProperty, History, Model};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use ts_rs::TS;

use crate::atomic;
use crate::dataview::{self, SharedIndexes, View, ViewSpec};
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
        if let Ok(text) = std::fs::read_to_string(&path)
            && let Ok(pid) = text.trim().parse::<u32>()
            && pid != std::process::id()
            && Path::new(&format!("/proc/{pid}")).exists()
        {
            return Err(CoreError::Locked { pid });
        }
        atomic::write(&path, std::process::id().to_string().as_bytes()).map_err(|source| {
            ProjectError::Io {
                path: path.clone(),
                source,
            }
        })?;
        Ok(Self { path })
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Loads one query's rows into its table.
pub fn load_query(db: &Database, query: &Query, project_dir: &str) -> Result<u64, CoreError> {
    let select = steps::to_sql(query, project_dir);
    db.materialise(&query.table, &select).map_err(|e| {
        CoreError::Invalid(format!(
            "could not load \"{}\" from {}: {e}",
            query.name,
            query.source.path()
        ))
    })
}

pub struct Session {
    pub project: Project,
    history: History,
    db: Database,
    indexes: SharedIndexes,
    schema: crate::visualq::Schema,
    /// What each calculated column was last computed from (expression, type, relationships).
    calc_state: std::collections::HashMap<String, String>,
    recovery: Option<RecoveryInfo>,
    _lock: Lock,
}

impl Session {
    pub fn open(path: &Path) -> Result<Self, CoreError> {
        let project = Project::open(path)?;
        let lock = Lock::acquire(&project.cache_dir())?;
        let db = Database::open(Some(&project.database_path()), &db_options(&project))?;
        dataview::attach(&db)?;
        for q in &project.queries {
            if !db.table_exists(&q.table)? {
                load_query(&db, q, &project.dir_str())?;
            }
        }
        let recovery = read_recovery(&project).and_then(|r| {
            (r.generation == project.manifest.generation && r.model != project.model).then_some(
                RecoveryInfo {
                    saved_at: r.saved_at,
                },
            )
        });
        let schema = crate::visualq::Schema::read(&db, &project.model);
        let mut session = Self {
            project,
            history: History::with_limit(500),
            db,
            indexes: SharedIndexes::default(),
            schema,
            calc_state: Default::default(),
            recovery,
            _lock: lock,
        };
        session.refresh_calculated();
        Ok(session)
    }

    pub fn database(&self) -> &Database {
        &self.db
    }

    /// Column types of the loaded tables.
    pub fn schema(&self) -> &crate::visualq::Schema {
        &self.schema
    }

    pub fn snapshot(&self) -> Result<ProjectSnapshot, CoreError> {
        let mut tables = Vec::new();
        for t in &self.project.model.tables {
            let rows = if self.db.table_exists(&t.source)? {
                self.db
                    .query_u64(&format!("SELECT count(*) FROM {}", sql::ident(&t.source)))?
            } else {
                0
            };
            tables.push(TableStats {
                id: t.id.clone(),
                rows,
            });
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
        let changes = self.prepare(changes)?;
        self.check_against_data(&changes)?;
        self.check_model(&changes)?;
        self.history
            .commit(&mut self.project.model, label, changes)?;
        self.refresh_calculated();
        self.write_recovery();
        Ok(())
    }

    pub fn undo(&mut self) -> Result<(), CoreError> {
        self.history.undo(&mut self.project.model)?;
        self.refresh_calculated();
        self.write_recovery();
        Ok(())
    }

    pub fn redo(&mut self) -> Result<(), CoreError> {
        self.history.redo(&mut self.project.model)?;
        self.refresh_calculated();
        self.write_recovery();
        Ok(())
    }

    /// Fills in what the client cannot know: a new calculated column's type.
    fn prepare(&self, mut changes: Vec<Change>) -> Result<Vec<Change>, CoreError> {
        for c in &mut changes {
            if let Change::AddColumn { table, column, .. } = c
                && let ColumnKind::Calculated { expression, .. } = &column.kind
                && column.data_type == charted_model::DataType::Unknown
            {
                let (_, ty) = charted_compiler::visual::calculated_column(
                    &self.project.model,
                    &self.schema,
                    table,
                    expression,
                )
                .map_err(|e| CoreError::Invalid(e.to_string()))?;
                column.data_type = charted_compiler::visual::value_type_to_data_type(ty);
            }
        }
        Ok(changes)
    }

    /// Model rules that need the whole model: no new ambiguous filter paths, and
    /// calculated columns that compile.
    fn check_model(&self, changes: &[Change]) -> Result<(), CoreError> {
        let mut after = self.project.model.clone();
        charted_model::apply_all(&mut after, changes)?;
        let before = charted_compiler::graph::ambiguities(&self.project.model);
        if let Some((a, b)) = charted_compiler::graph::ambiguities(&after)
            .into_iter()
            .find(|x| !before.contains(x))
        {
            let name = |id: &str| after.table(id).map(|t| t.name.clone()).unwrap_or_default();
            return Err(CoreError::Invalid(format!(
                "this would give filters two active paths from '{}' to '{}'; make one of the relationships inactive or filter in a single direction",
                name(&b),
                name(&a)
            )));
        }
        for c in changes {
            let (table, column) = match c {
                Change::AddColumn { table, column, .. } => (table, column.id.clone()),
                Change::SetColumn {
                    table,
                    column,
                    property: ColumnProperty::Expression(_),
                } => (table, column.clone()),
                _ => continue,
            };
            if let Some(col) = after.table(table).and_then(|t| t.column(&column))
                && let ColumnKind::Calculated { expression, .. } = &col.kind
            {
                charted_compiler::visual::calculated_column(
                    &after,
                    &self.schema,
                    table,
                    expression,
                )
                .map_err(|e| CoreError::Invalid(format!("{}: {e}", col.name)))?;
            }
        }
        Ok(())
    }

    /// Computes calculated columns whose expression, type or relationships changed,
    /// storing their values in a column of the table's data.
    fn refresh_calculated(&mut self) {
        let model = self.project.model.clone();
        let rels = serde_json::to_string(&model.relationships).unwrap_or_default();
        let mut pending: Vec<(
            &charted_model::Table,
            &charted_model::Column,
            String,
            String,
        )> = Vec::new();
        for t in &model.tables {
            for c in &t.columns {
                if let ColumnKind::Calculated { expression, .. } = &c.kind {
                    let sig = format!("{expression}\u{0}{:?}\u{0}{rels}", c.data_type);
                    if self.calc_state.get(&c.id) != Some(&sig) {
                        pending.push((t, c, expression.clone(), sig));
                    }
                }
            }
        }
        // Columns that use other calculated columns wait for them: retry while progressing.
        let mut changed = false;
        loop {
            let mut progress = false;
            let mut rest = Vec::new();
            for (t, c, expression, sig) in pending {
                match charted_compiler::visual::calculated_column(
                    &model,
                    &self.schema,
                    &t.id,
                    &expression,
                ) {
                    Ok((select, _)) => {
                        let col = sql::ident(&charted_compiler::calc_column_name(&c.id));
                        let src = sql::ident(&t.source);
                        let ty = charted_compiler::sqltype(c.data_type).unwrap_or("VARCHAR");
                        let script = format!(
                            "ALTER TABLE {src} DROP COLUMN IF EXISTS {col}; ALTER TABLE {src} ADD COLUMN {col} {ty}; \
                             UPDATE {src} SET {col} = TRY_CAST(s.v AS {ty}) FROM ({select}) AS s WHERE {src}.rowid = s.rid;"
                        );
                        match self.db.execute(&script) {
                            Ok(()) => {
                                self.calc_state.insert(c.id.clone(), sig);
                                // Later columns see this one.
                                self.schema = crate::visualq::Schema::read(&self.db, &model);
                                progress = true;
                                changed = true;
                            }
                            Err(e) => {
                                tracing::warn!("could not compute {}[{}]: {e}", t.name, c.name)
                            }
                        }
                    }
                    Err(_) => rest.push((t, c, expression, sig)),
                }
            }
            pending = rest;
            if !progress || pending.is_empty() {
                break;
            }
        }
        if changed {
            self.schema = crate::visualq::Schema::read(&self.db, &model);
        }
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
        let r = read_recovery(&self.project)
            .ok_or_else(|| CoreError::Invalid("there is nothing to restore".into()))?;
        let changes = whole_model_changes(&self.project.model, &r.model);
        self.history
            .commit(&mut self.project.model, "Restore unsaved changes", changes)?;
        self.refresh_calculated();
        self.recovery = None;
        self.write_recovery();
        Ok(())
    }

    pub fn discard_recovery(&mut self) {
        let _ = std::fs::remove_file(recovery_path(&self.project));
        self.recovery = None;
    }

    /// Resolves a Data view over a model table. The result runs its queries without
    /// borrowing the session, so callers can release their locks first.
    pub fn view(&self, spec: &ViewSpec) -> Result<View, CoreError> {
        let t = self
            .project
            .model
            .table(&spec.table)
            .ok_or_else(|| CoreError::Invalid(format!("unknown table {}", spec.table)))?;
        View::new(self.db.clone(), self.indexes.clone(), t, spec)
    }

    /// Changes that are valid for the model but not for the loaded data: a type the
    /// values don't convert to, or a sort-by column that isn't one value per value.
    fn check_against_data(&self, changes: &[Change]) -> Result<(), CoreError> {
        for c in changes {
            let Change::SetColumn {
                table,
                column,
                property,
            } = c
            else {
                continue;
            };
            let view = || {
                self.view(&ViewSpec {
                    table: table.clone(),
                    sort: Vec::new(),
                    filters: Vec::new(),
                })
            };
            match property {
                ColumnProperty::DataType(t) => view()?.check_type_change(column, *t)?,
                ColumnProperty::SortBy(Some(by)) => view()?.check_sort_by(column, by)?,
                _ => {}
            }
        }
        Ok(())
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
        let file = RecoveryFile {
            generation: self.project.manifest.generation,
            saved_at: now(),
            model: self.project.model.clone(),
        };
        if let Ok(json) = serde_json::to_vec(&file)
            && let Err(e) = atomic::write(&path, &json)
        {
            tracing::warn!("could not write recovery file {}: {e}", path.display());
        }
    }
}

fn db_options(project: &Project) -> DbOptions {
    DbOptions {
        temp_directory: Some(project.cache_dir().join("spill")),
        ..Default::default()
    }
}

fn recovery_path(project: &Project) -> PathBuf {
    project.cache_dir().join("recovery.json")
}

fn read_recovery(project: &Project) -> Option<RecoveryFile> {
    let text = std::fs::read_to_string(recovery_path(project)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Changes that turn `from` into `to` by replacing every table, relationship and page.
fn whole_model_changes(from: &Model, to: &Model) -> Vec<Change> {
    let mut changes: Vec<Change> = from
        .relationships
        .iter()
        .map(|r| Change::RemoveRelationship {
            relationship: r.id.clone(),
        })
        .collect();
    changes.extend(from.tables.iter().map(|t| Change::RemoveTable {
        table: t.id.clone(),
    }));
    changes.extend(to.tables.iter().map(|t| Change::AddTable {
        table: t.clone(),
        index: None,
    }));
    changes.extend(to.relationships.iter().map(|r| Change::AddRelationship {
        relationship: r.clone(),
        index: None,
    }));
    changes.extend(
        from.report
            .pages
            .iter()
            .map(|p| Change::RemovePage { page: p.id.clone() }),
    );
    changes.extend(to.report.pages.iter().map(|p| Change::AddPage {
        page: p.clone(),
        index: None,
    }));
    changes.push(Change::SetReportFilters {
        filters: to.report.filters.clone(),
    });
    changes
}
