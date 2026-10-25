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
    /// A source needs a DuckDB extension that must be downloaded first (its name).
    #[error(
        "this source needs DuckDB's {0} extension, which charted downloads from extensions.duckdb.org once you agree"
    )]
    Consent(String),
    /// A database password isn't in the keyring (the connection, as user@host:port/db).
    #[error("enter the password for {0}")]
    Password(String),
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
            CoreError::Consent(_) => "consent",
            CoreError::Password(_) => "password",
        }
    }

    /// What the error is about, when the UI needs it to offer a fix (the extension to
    /// download, the connection needing a password).
    pub fn detail(&self) -> Option<String> {
        match self {
            CoreError::Consent(x) | CoreError::Password(x) => Some(x.clone()),
            _ => None,
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
    pub definition: Query,
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

/// Loads one query's rows into its table. Only for queries that read a file directly
/// (the sample); a session loads through `Session::load`.
pub fn load_query(db: &Database, query: &Query, project_dir: &str) -> Result<u64, CoreError> {
    let sheets = Default::default();
    let ctx = steps::Ctx {
        project_dir,
        queries: std::slice::from_ref(query),
        sheets: &sheets,
    };
    let select = steps::to_sql(query, &ctx).map_err(|e| CoreError::Invalid(e.to_string()))?;
    materialise(db, query, &select)
}

fn materialise(db: &Database, query: &Query, select: &str) -> Result<u64, CoreError> {
    db.materialise(&query.table, select).map_err(|e| {
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
    /// Extensions the user agreed to download (set by the `App`).
    pub consent: crate::sources::Consent,
    /// Each file source's stamp when it was last loaded, by query id.
    stamps: crate::sources::Stamps,
    /// Cached CSV copies of the Excel sheets the queries read.
    sheets: std::collections::BTreeMap<String, String>,
    _lock: Lock,
}

impl Session {
    pub fn open(path: &Path) -> Result<Self, CoreError> {
        Self::open_with(path, crate::sources::Consent::default())
    }

    /// Opens a project, loading any table whose data isn't cached yet.
    pub fn open_with(path: &Path, consent: crate::sources::Consent) -> Result<Self, CoreError> {
        let project = Project::open(path)?;
        let lock = Lock::acquire(&project.cache_dir())?;
        let db = Database::open(Some(&project.database_path()), &db_options(&project))?;
        dataview::attach(&db)?;
        let mut stamps = crate::sources::read_stamps(&project.cache_dir());
        let missing: Vec<String> = project
            .queries
            .iter()
            .filter(|q| !q.no_load && !db.table_exists(&q.table).unwrap_or(false))
            .map(|q| q.id.clone())
            .collect();
        let dir = project.dir_str();
        // Sources loaded before charted kept stamps count as current.
        for q in &project.queries {
            if !stamps.contains_key(&q.id)
                && let Some(st) = crate::sources::full_stamp(&dir, &q.source)
            {
                stamps.insert(q.id.clone(), st);
            }
        }
        let sheets = if missing.is_empty() {
            crate::sources::prepare_sheets(&dir, &project.cache_dir(), &project.queries)
                .unwrap_or_default()
        } else {
            let sheets = crate::sources::prepare(
                &db,
                &dir,
                &project.cache_dir(),
                &project.queries,
                &consent,
            )?;
            for q in project.queries.iter().filter(|q| missing.contains(&q.id)) {
                let select = crate::sources::query_sql(&dir, &project.queries, &sheets, q, None)?;
                materialise(&db, q, &select)?;
                if let Some(st) = crate::sources::full_stamp(&dir, &q.source) {
                    stamps.insert(q.id.clone(), st);
                }
            }
            sheets
        };
        crate::sources::write_stamps(&project.cache_dir(), &stamps);
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
            consent,
            stamps,
            sheets,
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
                    source_path: q.source.path(),
                    sql: crate::sources::query_sql(
                        &self.project.dir_str(),
                        &self.project.queries,
                        &self.sheets,
                        q,
                        None,
                    )
                    .unwrap_or_else(|e| format!("-- {e}")),
                    definition: q.clone(),
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
    /// Gets the sources of a (draft) set of queries ready and returns the Excel sheets.
    pub fn prepare_sources(
        &self,
        queries: &[Query],
    ) -> Result<std::collections::BTreeMap<String, String>, CoreError> {
        crate::sources::prepare(
            &self.db,
            &self.project.dir_str(),
            &self.project.cache_dir(),
            queries,
            &self.consent,
        )
    }

    /// A query (from a draft set) up to `upto` steps, ready for the query editor.
    pub fn prepared(
        &self,
        query: &Query,
        queries: &[Query],
        upto: usize,
    ) -> Result<crate::queryedit::Prepared, CoreError> {
        let mut all: Vec<Query> = queries.to_vec();
        if !all.iter().any(|q| q.id == query.id) {
            all.push(query.clone());
        }
        let sheets = self.prepare_sources(&all)?;
        let dir = self.project.dir_str();
        let sql = crate::sources::query_sql(&dir, &all, &sheets, query, Some(upto))?;
        let type_change = crate::queryedit::type_change_before(query, upto, || {
            crate::sources::query_sql(&dir, &all, &sheets, query, Some(upto - 1))
        })?;
        Ok(crate::queryedit::Prepared {
            db: self.db.clone(),
            sql,
            type_change,
        })
    }

    /// Each file source compared with when it was last loaded.
    pub fn source_status(&self) -> Vec<crate::sources::SourceStatus> {
        crate::sources::status(&self.project.dir_str(), &self.project.queries, &self.stamps)
    }

    /// Replaces the project's queries (Close & Apply in the query editor): loads the
    /// queries that changed (and those reading them), then brings the model's tables
    /// and columns in line with the loaded data. The data can't be un-loaded, so the
    /// undo history is cleared.
    pub fn apply_queries(&mut self, queries: Vec<Query>) -> Result<(), CoreError> {
        let old = self.project.queries.clone();
        let mut ids: Vec<String> = queries
            .iter()
            .filter(|q| old.iter().find(|o| o.id == q.id) != Some(*q))
            .map(|q| q.id.clone())
            .collect();
        // Queries that read a changed query (merges, appends) change with it.
        loop {
            let more: Vec<String> = queries
                .iter()
                .filter(|q| !ids.contains(&q.id))
                .filter(|q| {
                    q.steps
                        .iter()
                        .any(|s| s.references().iter().any(|r| ids.iter().any(|i| i == r)))
                })
                .map(|q| q.id.clone())
                .collect();
            if more.is_empty() {
                break;
            }
            ids.extend(more);
        }
        let mut names = std::collections::HashSet::new();
        for q in &queries {
            if !names.insert(q.name.to_lowercase()) {
                return Err(CoreError::Invalid(format!(
                    "two queries are called \"{}\"",
                    q.name
                )));
            }
        }
        let removed: Vec<Query> = old
            .iter()
            .filter(|o| !queries.iter().any(|q| q.id == o.id))
            .cloned()
            .collect();
        self.load_into_model(&queries, &ids, &removed, "Apply query changes")
    }

    /// The date table wizard: a generated calendar query loaded as a table, marked as
    /// the date table, its month names sorted by month number, and related to the
    /// given date columns (the first per table active). Clears undo, as Apply does.
    pub fn create_date_table(
        &mut self,
        name: &str,
        start: &str,
        end: &str,
        fiscal_start: u32,
        relate: &[(charted_model::Id, charted_model::Id)],
    ) -> Result<(), CoreError> {
        use charted_compiler::steps::Source;
        let name = name.trim();
        if name.is_empty() {
            return Err(CoreError::Invalid("the date table needs a name".into()));
        }
        let model = &self.project.model;
        if model.table_by_name(name).is_some()
            || self
                .project
                .queries
                .iter()
                .any(|q| q.name.eq_ignore_ascii_case(name))
        {
            return Err(CoreError::Invalid(format!(
                "there's already a table called \"{name}\""
            )));
        }
        let base: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect();
        let mut source_table = base.clone();
        let mut k = 2;
        while self.project.queries.iter().any(|q| q.table == source_table)
            || model.tables.iter().any(|t| t.source == source_table)
        {
            source_table = format!("{base}_{k}");
            k += 1;
        }
        for (t, c) in relate {
            let col = model.table(t).and_then(|x| x.column(c)).ok_or_else(|| {
                CoreError::Invalid("a column to relate to the date table is missing".into())
            })?;
            if col.data_type != charted_model::DataType::Date {
                return Err(CoreError::Invalid(format!(
                    "[{}] isn't a date column (a date table relates to columns of type Date)",
                    col.name
                )));
            }
        }
        let q = Query {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            table: source_table.clone(),
            source: Source::Calendar {
                start: start.to_string(),
                end: end.to_string(),
                fiscal_start,
            },
            steps: Vec::new(),
            step_names: Default::default(),
            no_load: false,
        };
        let mut queries = self.project.queries.clone();
        queries.push(q.clone());
        self.load_into_model(&queries, std::slice::from_ref(&q.id), &[], "New date table")?;
        let model = &self.project.model;
        let table = model
            .tables
            .iter()
            .find(|t| t.source == source_table)
            .ok_or_else(|| CoreError::Invalid("the date table didn't load".into()))?;
        let col = |n: &str| table.column_by_name(n).map(|c| c.id.clone());
        let date = col("Date")
            .ok_or_else(|| CoreError::Invalid("the date table has no Date column".into()))?;
        let mut changes = vec![
            Change::SetDateTable {
                table: table.id.clone(),
                is_date_table: true,
            },
            Change::SetColumn {
                table: table.id.clone(),
                column: date.clone(),
                property: charted_model::ColumnProperty::IsKey(true),
            },
        ];
        for (shown, by) in [("MonthName", "MonthNumber"), ("Weekday", "WeekdayNumber")] {
            if let (Some(a), Some(b)) = (col(shown), col(by)) {
                changes.push(Change::SetColumn {
                    table: table.id.clone(),
                    column: a,
                    property: charted_model::ColumnProperty::SortBy(Some(b)),
                });
            }
        }
        let mut related: Vec<&charted_model::Id> = Vec::new();
        for (t, c) in relate {
            changes.push(Change::AddRelationship {
                relationship: charted_model::Relationship {
                    id: uuid::Uuid::new_v4().to_string(),
                    from_table: t.clone(),
                    from_column: c.clone(),
                    to_table: table.id.clone(),
                    to_column: date.clone(),
                    from_cardinality: charted_model::Cardinality::Many,
                    to_cardinality: charted_model::Cardinality::One,
                    cross_filter: charted_model::CrossFilter::Single,
                    active: !related.contains(&t),
                    extras: Default::default(),
                },
                index: None,
            });
            related.push(t);
        }
        self.commit("New date table", changes)?;
        self.history.forget();
        self.write_recovery();
        Ok(())
    }

    /// Loads every query again from its sources (Refresh).
    pub fn refresh(&mut self) -> Result<(), CoreError> {
        let queries = self.project.queries.clone();
        let ids = queries.iter().map(|q| q.id.clone()).collect::<Vec<_>>();
        self.load_into_model(&queries, &ids, &[], "Refresh")
    }

    fn load_into_model(
        &mut self,
        queries: &[Query],
        ids: &[String],
        removed: &[Query],
        label: &str,
    ) -> Result<(), CoreError> {
        let dir = self.project.dir_str();
        let sheets = self.prepare_sources(queries)?;
        // Load into staging tables first, so a failure leaves everything as it was.
        let mut staged = Vec::new();
        let result = (|| {
            for (i, q) in queries
                .iter()
                .filter(|q| ids.contains(&q.id) && !q.no_load)
                .enumerate()
            {
                let select = crate::sources::query_sql(&dir, queries, &sheets, q, None)?;
                let tmp = format!("__staging_{i}");
                self.db.materialise(&tmp, &select).map_err(|e| {
                    CoreError::Invalid(format!(
                        "could not load \"{}\" from {}: {e}",
                        q.name,
                        q.source.path()
                    ))
                })?;
                staged.push((tmp, q.table.clone()));
            }
            Ok::<(), CoreError>(())
        })();
        if let Err(e) = result {
            for (tmp, _) in &staged {
                let _ = self
                    .db
                    .execute(&format!("DROP TABLE IF EXISTS {}", sql::ident(tmp)));
            }
            return Err(e);
        }
        for (tmp, table) in &staged {
            self.db.execute(&format!(
                "DROP TABLE IF EXISTS {t}; ALTER TABLE {} RENAME TO {t}",
                sql::ident(tmp),
                t = sql::ident(table)
            ))?;
        }
        let gone: Vec<&Query> = removed
            .iter()
            .chain(queries.iter().filter(|q| q.no_load))
            .collect();
        for q in &gone {
            if !queries.iter().any(|x| x.table == q.table && !x.no_load) {
                self.db
                    .execute(&format!("DROP TABLE IF EXISTS {}", sql::ident(&q.table)))?;
            }
        }
        let changes = self.model_sync(queries, ids, &gone)?;
        self.project.queries = queries.to_vec();
        self.sheets = sheets;
        for q in queries.iter().filter(|q| ids.contains(&q.id)) {
            match crate::sources::full_stamp(&dir, &q.source) {
                Some(st) => self.stamps.insert(q.id.clone(), st),
                None => self.stamps.remove(&q.id),
            };
        }
        for q in removed {
            self.stamps.remove(&q.id);
        }
        crate::sources::write_stamps(&self.project.cache_dir(), &self.stamps);
        // The loaded tables are new: their views and calculated columns start again.
        self.indexes = SharedIndexes::default();
        self.calc_state.clear();
        self.schema = crate::visualq::Schema::read(&self.db, &self.project.model);
        if !changes.is_empty() {
            self.commit(label, changes)?;
        } else {
            self.refresh_calculated();
        }
        self.history.forget();
        self.write_recovery();
        Ok(())
    }

    /// Changes bringing the model's tables in line with the loaded queries: new tables,
    /// added and removed columns, changed types, renamed queries; tables of removed
    /// queries go.
    fn model_sync(
        &self,
        queries: &[Query],
        ids: &[String],
        gone: &[&Query],
    ) -> Result<Vec<Change>, CoreError> {
        use charted_model::{Column, ColumnProperty, Summarize, Table};
        let model = &self.project.model;
        let old = &self.project.queries;
        let mut changes = Vec::new();
        for q in gone {
            if queries.iter().any(|x| x.table == q.table && !x.no_load) {
                continue;
            }
            if let Some(t) = model.tables.iter().find(|t| t.source == q.table) {
                changes.push(Change::RemoveTable {
                    table: t.id.clone(),
                });
            }
        }
        let new_column = |name: &str, data_type| Column {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            data_type,
            kind: ColumnKind::Data {
                source_column: name.to_string(),
            },
            description: None,
            format_string: None,
            summarize_by: if name.ends_with("Key")
                || name.ends_with("ID")
                || name.ends_with("Id")
                || name == "Year"
            {
                Summarize::None
            } else {
                Summarize::Default
            },
            data_category: None,
            sort_by: None,
            display_folder: None,
            hidden: false,
            is_key: false,
            extras: Default::default(),
        };
        let mut taken: Vec<String> = model.tables.iter().map(|t| t.name.to_lowercase()).collect();
        for q in queries.iter().filter(|q| ids.contains(&q.id) && !q.no_load) {
            let cols = self.db.describe(&sql::ident(&q.table))?;
            match model.tables.iter().find(|t| t.source == q.table) {
                None => {
                    let mut name = q.name.clone();
                    let mut k = 2;
                    while taken.contains(&name.to_lowercase()) {
                        name = format!("{} {k}", q.name);
                        k += 1;
                    }
                    taken.push(name.to_lowercase());
                    changes.push(Change::AddTable {
                        table: Table {
                            id: uuid::Uuid::new_v4().to_string(),
                            name,
                            description: None,
                            source: q.table.clone(),
                            columns: cols
                                .iter()
                                .map(|c| {
                                    new_column(&c.name, crate::types::from_duckdb(&c.duckdb_type))
                                })
                                .collect(),
                            measures: vec![],
                            hidden: false,
                            is_date_table: false,
                            extras: Default::default(),
                        },
                        index: None,
                    });
                }
                Some(t) => {
                    // A renamed query renames its table.
                    if let Some(o) = old.iter().find(|o| o.id == q.id)
                        && o.name != q.name
                        && t.name == o.name
                        && !model
                            .tables
                            .iter()
                            .any(|x| x.id != t.id && x.name.eq_ignore_ascii_case(&q.name))
                    {
                        changes.push(Change::RenameTable {
                            table: t.id.clone(),
                            name: q.name.clone(),
                        });
                    }
                    let physical: Vec<(String, charted_model::DataType)> = cols
                        .iter()
                        .map(|c| (c.name.clone(), crate::types::from_duckdb(&c.duckdb_type)))
                        .collect();
                    for c in &t.columns {
                        let ColumnKind::Data { source_column } = &c.kind else {
                            continue;
                        };
                        match physical.iter().find(|(n, _)| n == source_column) {
                            None => {
                                // What uses the column goes first.
                                for r in model
                                    .relationships
                                    .iter()
                                    .filter(|r| r.from_column == c.id || r.to_column == c.id)
                                {
                                    if !changes.iter().any(|x| matches!(x, Change::RemoveRelationship { relationship } if *relationship == r.id)) {
                                        changes.push(Change::RemoveRelationship { relationship: r.id.clone() });
                                    }
                                }
                                for other in t
                                    .columns
                                    .iter()
                                    .filter(|o| o.sort_by.as_deref() == Some(c.id.as_str()))
                                {
                                    changes.push(Change::SetColumn {
                                        table: t.id.clone(),
                                        column: other.id.clone(),
                                        property: ColumnProperty::SortBy(None),
                                    });
                                }
                                changes.push(Change::RemoveColumn {
                                    table: t.id.clone(),
                                    column: c.id.clone(),
                                });
                            }
                            Some((_, ty))
                                if *ty != c.data_type
                                    && c.data_type != charted_model::DataType::Unknown =>
                            {
                                changes.push(Change::SetColumn {
                                    table: t.id.clone(),
                                    column: c.id.clone(),
                                    property: ColumnProperty::DataType(*ty),
                                });
                            }
                            _ => {}
                        }
                    }
                    for (name, ty) in &physical {
                        let known = t.columns.iter().any(|c| matches!(&c.kind, ColumnKind::Data { source_column } if source_column == name));
                        if !known && !name.starts_with("__calc_") {
                            let mut col = new_column(name, *ty);
                            // Keep column names unique next to calculated columns.
                            if t.columns.iter().any(|c| c.name.eq_ignore_ascii_case(name)) {
                                col.name = format!("{name} (data)");
                            }
                            changes.push(Change::AddColumn {
                                table: t.id.clone(),
                                column: col,
                                index: None,
                            });
                        }
                    }
                }
            }
        }
        Ok(changes)
    }

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

    /// Replaces the model with an Ossie document's model, as one undoable step.
    pub fn import_ossie(&mut self, file: &Path) -> Result<(), CoreError> {
        let (model, _) = crate::interop::import_diff(&self.project.model, file)?;
        let changes = whole_model_changes(&self.project.model, &model);
        self.history.commit(
            &mut self.project.model,
            "Import Apache Ossie model",
            changes,
        )?;
        self.refresh_calculated();
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
