//! The request/response API. The desktop shell, the command line and the development
//! WebSocket server all call `App::handle`, so they can never drift apart.

use charted_compiler::steps::Query;
use charted_compiler::visual::{QueryFilter, VisualQuery};
use charted_engine::Database;
use charted_model::{Change, Id, Projection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use ts_rs::TS;

use crate::dataview::{View, ViewSpec};
use crate::recent::{self, RecentProject};
use crate::session::{CoreError, ProjectSnapshot, Session};
use crate::visualq;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum Request {
    /// Current state: the open project (if any) and recent projects.
    State,
    OpenProject {
        path: String,
    },
    /// Opens the sample project, generating it first if needed. Without `dir` it lives
    /// in the user's documents folder under charted/.
    OpenSample {
        dir: Option<String>,
    },
    CloseProject,
    Commit {
        label: String,
        changes: Vec<Change>,
    },
    Undo,
    Redo,
    Save,
    RestoreRecovery,
    DiscardRecovery,
    ForgetRecent {
        path: String,
    },
    /// Writes a Power BI project (PBIP with TMDL) into a folder.
    ExportPbip {
        dir: String,
    },
    /// Writes the model as an Apache Ossie document.
    ExportOssie {
        path: String,
    },
    /// Replaces the model with an Ossie document's (one undoable step).
    ImportOssie {
        path: String,
    },
    /// Imports a Power BI project (.pbip, or a model.bim) as a new project in a new
    /// folder under `dir` (the documents folder's charted folder by default), and opens
    /// it with its import report.
    ImportPowerBi {
        path: String,
        dir: Option<String>,
    },
    /// Hides the import report.
    DismissImportReport,
    /// Writes the project as a .chartedz bundle (saved first if needed).
    SaveBundle {
        path: String,
    },
    /// Replaces the project's queries with the query editor's (Close & Apply): loads
    /// what changed and brings the model's tables in line. Clears undo.
    ApplyQueries {
        queries: Vec<Query>,
    },
    /// Loads every query again from its sources. Clears undo.
    Refresh,
    /// The date table wizard (see `Session::create_date_table`). Clears undo.
    CreateDateTable {
        name: String,
        /// ISO dates (YYYY-MM-DD).
        start: String,
        end: String,
        /// The month the fiscal year starts in (1: calendar years).
        fiscal_start: u32,
        /// Date columns to relate to it: (table id, column id).
        relate: Vec<(Id, Id)>,
    },
    /// Records the user's agreement to download a DuckDB extension (and downloads it).
    AllowExtension {
        name: String,
    },
    /// Stores a PostgreSQL password in the desktop keyring.
    SetPassword {
        host: String,
        port: u16,
        database: String,
        user: String,
        password: String,
    },
    /// Sets the report theme from a Power BI theme file (one undoable step).
    ImportTheme {
        path: String,
    },
    /// Writes a report visual's data as CSV, named after its fields, with `filters`
    /// added (a selection elsewhere filtering it).
    ExportVisualCsv {
        visual: Id,
        path: String,
        #[serde(default)]
        filters: Vec<QueryFilter>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AppState {
    pub project: Option<ProjectSnapshot>,
    pub recent: Vec<RecentProject>,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    /// What the error is about, when the UI can offer a fix (see `CoreError::detail`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl From<CoreError> for ApiError {
    fn from(e: CoreError) -> Self {
        Self {
            code: e.code().into(),
            detail: e.detail(),
            message: e.to_string(),
        }
    }
}

/// Requests that read data, kept apart from `Request` so large results skip JSON and
/// slow queries never hold up model edits. `Rows` answers with Arrow IPC; the others
/// answer with JSON bytes (`ViewCount`, `DistinctValues`, `ColumnStats`,
/// `FindResult | null`).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum DataRequest {
    Rows {
        view: ViewSpec,
        /// Only these columns (all when omitted).
        #[serde(default)]
        columns: Option<Vec<Id>>,
        #[ts(type = "number")]
        offset: u64,
        limit: u32,
    },
    Count {
        view: ViewSpec,
        #[serde(default)]
        column: Option<Id>,
    },
    Distinct {
        view: ViewSpec,
        column: Id,
        #[serde(default)]
        search: Option<String>,
        limit: u32,
    },
    Stats {
        view: ViewSpec,
        column: Id,
    },
    Find {
        view: ViewSpec,
        text: String,
        #[serde(default)]
        columns: Option<Vec<Id>>,
        /// Search after this row (before it when `backwards`); from the start if omitted.
        #[serde(default)]
        #[ts(type = "number | null")]
        after: Option<u64>,
        #[serde(default)]
        backwards: bool,
    },
    /// A visual query's data (Arrow): groups `g0…` then values `v0…`.
    Visual {
        query: VisualQuery,
    },
    /// A report visual's data (Arrow); see `visualq::report_visual_sql` for its
    /// columns. `totals` drops the grouping (a table's total row); `filters` are added
    /// and `highlight` computes highlighted values (a selection in another visual). A
    /// new request with the same `tag` cancels one still running.
    ReportVisual {
        visual: Id,
        #[serde(default)]
        totals: bool,
        #[serde(default)]
        filters: Vec<QueryFilter>,
        #[serde(default)]
        highlight: Vec<QueryFilter>,
        #[serde(default)]
        tag: Option<String>,
    },
    /// Values of a column for a filter card, under the other filters (JSON
    /// `DistinctValues`, counts are zero).
    FieldValues {
        field: Projection,
        #[serde(default)]
        filters: Vec<QueryFilter>,
        #[serde(default)]
        search: Option<String>,
        limit: u32,
    },
    /// Checks a measure expression (JSON `DaxCheck`). `measure` is the measure being
    /// edited, if it exists.
    CheckDax {
        expression: String,
        #[serde(default)]
        measure: Option<Id>,
    },
    /// What importing an Ossie document would change (JSON `ModelDiff`).
    OssieDiff {
        path: String,
    },
    /// The bytes of an image file to place on a report page (PNG, JPEG, GIF, WebP or
    /// SVG, at most 5 MB).
    ReadImage {
        path: String,
    },
    /// The query editor's preview of a query at a step (Arrow, at most `limit` rows).
    /// `queries` is the editor's draft set (merges and appends read them).
    QueryPreview {
        query: Query,
        #[serde(default)]
        queries: Vec<Query>,
        upto: usize,
        limit: u32,
    },
    /// Columns, SQL and M of a query at a step (JSON `QueryInfo`).
    QueryInfo {
        query: Query,
        #[serde(default)]
        queries: Vec<Query>,
        upto: usize,
    },
    /// Column quality and distribution at a step (JSON `QueryProfile`): the first 1,000
    /// rows, or all of them with `full`.
    QueryProfile {
        query: Query,
        #[serde(default)]
        queries: Vec<Query>,
        upto: usize,
        #[serde(default)]
        full: bool,
    },
    /// A lookup made while adding a step (JSON; see `Analysis`).
    QueryAnalyze {
        query: Query,
        #[serde(default)]
        queries: Vec<Query>,
        upto: usize,
        analysis: crate::queryedit::Analysis,
    },
    /// The sheets of an Excel workbook (JSON list of names).
    ExcelSheets {
        path: String,
    },
    /// Each file source compared with when it was loaded (JSON `SourceStatus[]`).
    SourceStatus,
    /// Key counts and a suggested cardinality for a prospective relationship (JSON
    /// `RelationshipCheck`).
    CheckRelationship {
        from_table: Id,
        from_column: Id,
        to_table: Id,
        to_column: Id,
    },
}

fn json<T: Serialize>(value: Result<T, CoreError>) -> Result<Vec<u8>, ApiError> {
    Ok(serde_json::to_vec(&value?).unwrap_or_default())
}

/// A data request resolved against the open project, runnable without the `App`.
pub enum DataJob {
    View(Box<View>, Box<DataRequest>),
    Arrow(Database, String),
    /// Query editor work on a prepared query.
    Query(Box<QueryJob>),
    /// A query that a later one with the same tag cancels.
    Tagged(Database, String, Running, String),
    FieldValues(Database, String),
    Relationship(Database, String),
    Ready(Vec<u8>),
}

pub enum QueryJob {
    Preview(crate::queryedit::Prepared, u32),
    Info(crate::queryedit::Prepared, Box<Query>, Vec<Query>),
    Profile(crate::queryedit::Prepared, bool),
    Analyze(crate::queryedit::Prepared, crate::queryedit::Analysis),
}

/// Data jobs running now, in every `App` (see `shutdown`).
static IN_FLIGHT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Tagged queries of every `App` in the process, so `shutdown` can interrupt them.
static RUNNING: std::sync::LazyLock<Running> = std::sync::LazyLock::new(Running::default);

/// Prepares the process to exit: interrupts running tagged queries and waits (up to
/// `timeout`) for every data job to finish. DuckDB's static data is destroyed when the
/// process exits, so a query still binding on another thread at that moment crashes.
pub fn shutdown(timeout: std::time::Duration) {
    SHUTTING_DOWN.store(true, std::sync::atomic::Ordering::SeqCst);
    for (_, handle) in RUNNING.lock().values() {
        handle.interrupt();
    }
    let start = std::time::Instant::now();
    while IN_FLIGHT.load(std::sync::atomic::Ordering::SeqCst) > 0 && start.elapsed() < timeout {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// Set by `shutdown`: no new data jobs start.
static SHUTTING_DOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Counts a data job as in flight while it lives.
struct InFlight;

impl InFlight {
    fn new() -> Self {
        IN_FLIGHT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        IN_FLIGHT.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }
}

impl DataJob {
    pub fn run(self) -> Result<Vec<u8>, ApiError> {
        let _in_flight = InFlight::new();
        if SHUTTING_DOWN.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(ApiError {
                code: "cancelled".into(),
                message: "charted is closing".into(),
                detail: None,
            });
        }
        let (v, request) = match self {
            DataJob::View(v, r) => (v, r),
            DataJob::Arrow(db, sql) => return Ok(db.arrow(&sql).map_err(CoreError::from)?),
            DataJob::Tagged(db, sql, running, tag) => {
                let id = running
                    .next
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let result = db.arrow_interruptible(&sql, |handle| {
                    let mut m = running.lock();
                    if let Some((_, old)) = m.insert(tag.clone(), (id, handle)) {
                        old.interrupt();
                    }
                });
                let mut m = running.lock();
                if m.get(&tag).is_some_and(|(i, _)| *i == id) {
                    m.remove(&tag);
                }
                drop(m);
                return result.map_err(|e| {
                    if e.to_string().contains("INTERRUPT") {
                        ApiError {
                            code: "cancelled".into(),
                            message: "a newer request replaced this one".into(),
                            detail: None,
                        }
                    } else {
                        CoreError::from(e).into()
                    }
                });
            }
            DataJob::FieldValues(db, sql) => return json(visualq::run_field_values(&db, &sql)),
            DataJob::Relationship(db, sql) => return json(crate::relcheck::run(&db, &sql)),
            DataJob::Ready(bytes) => return Ok(bytes),
            DataJob::Query(job) => {
                use crate::queryedit as qe;
                return match *job {
                    QueryJob::Preview(p, limit) => Ok(qe::preview(&p, limit)?),
                    QueryJob::Info(p, q, all) => json(qe::info(&p, &q, &all)),
                    QueryJob::Profile(p, full) => json(qe::profile(&p, full)),
                    QueryJob::Analyze(p, a) => json(qe::analyze(&p, &a)),
                };
            }
        };
        let v = &*v;
        match *request {
            DataRequest::Rows {
                columns,
                offset,
                limit,
                ..
            } => Ok(v.rows(columns.as_deref(), offset, limit)?),
            DataRequest::Count { column, .. } => json(v.count(column.as_deref())),
            DataRequest::Distinct {
                column,
                search,
                limit,
                ..
            } => json(v.distinct(&column, search.as_deref(), limit)),
            DataRequest::Stats { column, .. } => json(v.stats(&column)),
            DataRequest::Find {
                text,
                columns,
                after,
                backwards,
                ..
            } => json(v.find(&text, columns.as_deref(), after, backwards)),
            _ => unreachable!("only view requests reach a view"),
        }
    }
}

pub struct App {
    session: Option<Session>,
    state_dir: PathBuf,
    running: Running,
}

type RunningMap = HashMap<String, (u64, Arc<charted_engine::InterruptHandle>)>;

/// Tagged queries still running, with the handle that interrupts each.
#[derive(Clone, Default)]
pub struct Running {
    queries: Arc<Mutex<RunningMap>>,
    next: Arc<std::sync::atomic::AtomicU64>,
}

impl Running {
    fn lock(&self) -> MutexGuard<'_, RunningMap> {
        self.queries.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl App {
    /// `state_dir` holds per-user state such as the recent-projects list.
    pub fn new(state_dir: PathBuf) -> Self {
        Self {
            session: None,
            state_dir,
            running: RUNNING.clone(),
        }
    }

    pub fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    pub fn session_mut(&mut self) -> Result<&mut Session, CoreError> {
        self.session.as_mut().ok_or(CoreError::NoProject)
    }

    pub fn is_dirty(&self) -> bool {
        self.session.as_ref().is_some_and(Session::is_dirty)
    }

    pub fn open(&mut self, path: &Path) -> Result<(), CoreError> {
        // Release the current project's lock before taking the next one.
        self.session = None;
        let session = Session::open_with(path, crate::sources::Consent::read(&self.state_dir))?;
        recent::remember(
            &self.state_dir,
            &session.project.manifest.name,
            &session.project.manifest_path,
        );
        self.session = Some(session);
        Ok(())
    }

    /// Imports a Power BI project as a new project under `parent` and opens it with
    /// its import report.
    fn import_powerbi(&mut self, path: &Path, parent: &Path) -> Result<(), CoreError> {
        let (project, report) = crate::pbimport::import(path, parent)?;
        self.open(&project)?;
        if let Some(s) = self.session.as_mut() {
            s.import_report = Some(report);
        }
        Ok(())
    }

    pub fn state(&self) -> Result<AppState, CoreError> {
        Ok(AppState {
            project: self.session.as_ref().map(Session::snapshot).transpose()?,
            recent: recent::list(&self.state_dir),
            version: env!("CARGO_PKG_VERSION").into(),
        })
    }

    pub fn handle(&mut self, request: Request) -> Result<AppState, ApiError> {
        self.apply(request)?;
        Ok(self.state()?)
    }

    fn apply(&mut self, request: Request) -> Result<(), CoreError> {
        match request {
            Request::State => {}
            Request::OpenProject { path } => {
                // A bundle opens as a new folder; a Power BI project is imported.
                let p = Path::new(&path);
                match p
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(str::to_ascii_lowercase)
                    .as_deref()
                {
                    Some(crate::bundle::EXTENSION) => {
                        let dir = crate::bundle::unpack(p, &projects_dir())?;
                        self.open(&dir)?;
                    }
                    Some("pbip" | "bim") => self.import_powerbi(p, &projects_dir())?,
                    _ => self.open(p)?,
                }
            }
            Request::OpenSample { dir } => {
                let dir = dir.map(PathBuf::from).unwrap_or_else(default_sample_dir);
                if crate::project::locate(&dir).is_err() {
                    crate::sample::create(&dir, charted_samplegen::Scale::Demo)?;
                }
                self.open(&dir)?;
            }
            Request::CloseProject => self.session = None,
            Request::Commit { label, changes } => self.session_mut()?.commit(&label, changes)?,
            Request::Undo => self.session_mut()?.undo()?,
            Request::Redo => self.session_mut()?.redo()?,
            Request::Save => self.session_mut()?.save()?,
            Request::RestoreRecovery => self.session_mut()?.restore_recovery()?,
            Request::DiscardRecovery => self.session_mut()?.discard_recovery(),
            Request::ForgetRecent { path } => recent::forget(&self.state_dir, Path::new(&path)),
            Request::ExportPbip { dir } => {
                let s = self.session_mut()?;
                crate::interop::export_pbip(&s.project, Path::new(&dir))?;
            }
            Request::ExportOssie { path } => {
                let s = self.session_mut()?;
                crate::interop::export_ossie(&s.project.model, Path::new(&path))?;
            }
            Request::ImportOssie { path } => self.session_mut()?.import_ossie(Path::new(&path))?,
            Request::ImportPowerBi { path, dir } => {
                let parent = dir.map(PathBuf::from).unwrap_or_else(projects_dir);
                self.import_powerbi(Path::new(&path), &parent)?;
            }
            Request::DismissImportReport => self.session_mut()?.import_report = None,
            Request::SaveBundle { path } => {
                let s = self.session_mut()?;
                s.save()?;
                crate::bundle::write(&s.project.dir, Path::new(&path))?;
            }
            Request::ApplyQueries { queries } => self.session_mut()?.apply_queries(queries)?,
            Request::Refresh => self.session_mut()?.refresh()?,
            Request::CreateDateTable {
                name,
                start,
                end,
                fiscal_start,
                relate,
            } => {
                self.session_mut()?
                    .create_date_table(&name, &start, &end, fiscal_start, &relate)?
            }
            Request::AllowExtension { name } => {
                let consent = crate::sources::Consent::allow(&self.state_dir, &name)?;
                if let Ok(s) = self.session_mut() {
                    s.consent = consent;
                    s.database()
                        .execute(&format!("INSTALL {}", charted_engine::sql::ident(&name)))
                        .map_err(|e| {
                            CoreError::Invalid(format!(
                                "couldn't download DuckDB's {name} extension: {e}"
                            ))
                        })?;
                }
            }
            Request::SetPassword {
                host,
                port,
                database,
                user,
                password,
            } => crate::sources::set_password(&host, port, &database, &user, &password)?,
            Request::ImportTheme { path } => {
                let theme = crate::reporttheme::read(Path::new(&path))?;
                self.session_mut()?.commit(
                    "Import theme",
                    vec![charted_model::Change::SetTheme { theme: Some(theme) }],
                )?;
            }
            Request::ExportVisualCsv {
                visual,
                path,
                filters,
            } => {
                let s = self.session_mut()?;
                let sql = visualq::report_visual_sql(
                    &s.project.model,
                    s.schema(),
                    &visual,
                    &visualq::VisualSqlOptions {
                        filters: &filters,
                        captions: true,
                        ..Default::default()
                    },
                )?;
                // Written beside the target and moved into place, so the file is
                // never seen half-written.
                let part = format!("{path}.part");
                s.database()
                    .execute(&format!(
                        "COPY ({sql}) TO {} (FORMAT csv, HEADER true)",
                        charted_engine::sql::string(&part)
                    ))
                    .map_err(CoreError::from)?;
                std::fs::rename(&part, &path).map_err(|e| {
                    let _ = std::fs::remove_file(&part);
                    CoreError::Invalid(format!("couldn't write {path}: {e}"))
                })?;
            }
        }
        Ok(())
    }

    /// Resolves a data request; run the job after releasing any lock on the `App`.
    pub fn data_job(&self, request: DataRequest) -> Result<DataJob, ApiError> {
        let s = self.session.as_ref().ok_or(CoreError::NoProject)?;
        let model = &s.project.model;
        let view = match &request {
            DataRequest::Rows { view, .. }
            | DataRequest::Count { view, .. }
            | DataRequest::Distinct { view, .. }
            | DataRequest::Stats { view, .. }
            | DataRequest::Find { view, .. } => s.view(view)?,
            DataRequest::Visual { query } => {
                let c = visualq::compile(model, s.schema(), query)?;
                return Ok(DataJob::Arrow(s.database().clone(), c.sql));
            }
            DataRequest::ReportVisual {
                visual,
                totals,
                filters,
                highlight,
                tag,
            } => {
                let sql = visualq::report_visual_sql(
                    model,
                    s.schema(),
                    visual,
                    &visualq::VisualSqlOptions {
                        totals: *totals,
                        filters,
                        highlight,
                        captions: false,
                    },
                )?;
                let db = s.database().clone();
                return Ok(match tag {
                    Some(tag) => DataJob::Tagged(db, sql, self.running.clone(), tag.clone()),
                    None => DataJob::Arrow(db, sql),
                });
            }
            DataRequest::FieldValues {
                field,
                filters,
                search,
                limit,
            } => {
                let sql = visualq::field_values_sql(
                    model,
                    s.schema(),
                    field,
                    filters,
                    search.as_deref(),
                    *limit,
                )?;
                return Ok(DataJob::FieldValues(s.database().clone(), sql));
            }
            DataRequest::CheckRelationship {
                from_table,
                from_column,
                to_table,
                to_column,
            } => {
                let sql =
                    crate::relcheck::plan(model, from_table, from_column, to_table, to_column)?;
                return Ok(DataJob::Relationship(s.database().clone(), sql));
            }
            DataRequest::ReadImage { path } => return Ok(DataJob::Ready(read_image(path)?)),
            DataRequest::QueryPreview {
                query,
                queries,
                upto,
                limit,
            } => {
                let p = s.prepared(query, queries, *upto)?;
                return Ok(DataJob::Query(Box::new(QueryJob::Preview(p, *limit))));
            }
            DataRequest::QueryInfo {
                query,
                queries,
                upto,
            } => {
                let p = s.prepared(query, queries, *upto)?;
                return Ok(DataJob::Query(Box::new(QueryJob::Info(
                    p,
                    Box::new(query.clone()),
                    queries.clone(),
                ))));
            }
            DataRequest::QueryProfile {
                query,
                queries,
                upto,
                full,
            } => {
                let p = s.prepared(query, queries, *upto)?;
                return Ok(DataJob::Query(Box::new(QueryJob::Profile(p, *full))));
            }
            DataRequest::QueryAnalyze {
                query,
                queries,
                upto,
                analysis,
            } => {
                let p = s.prepared(query, queries, *upto)?;
                return Ok(DataJob::Query(Box::new(QueryJob::Analyze(
                    p,
                    analysis.clone(),
                ))));
            }
            DataRequest::ExcelSheets { path } => {
                let p = charted_compiler::steps::resolve_path(&s.project.dir_str(), path);
                return Ok(DataJob::Ready(json(crate::sources::excel_sheets(
                    Path::new(&p),
                ))?));
            }
            DataRequest::SourceStatus => {
                return Ok(DataJob::Ready(json(Ok::<_, CoreError>(s.source_status()))?));
            }
            DataRequest::OssieDiff { path } => {
                let d = crate::interop::import_diff(model, Path::new(path))
                    .map(|(_, d)| d)
                    .map_err(CoreError::from);
                return Ok(DataJob::Ready(json(d)?));
            }
            DataRequest::CheckDax {
                expression,
                measure,
            } => {
                let check = visualq::check_dax(model, s.schema(), expression, measure.as_deref());
                return Ok(DataJob::Ready(json(Ok::<_, CoreError>(check))?));
            }
        };
        Ok(DataJob::View(Box::new(view), Box::new(request)))
    }

    pub fn data(&self, request: DataRequest) -> Result<Vec<u8>, ApiError> {
        self.data_job(request)?.run()
    }
}

/// Images placed on report pages are stored in the report, so they are kept small.
const MAX_IMAGE_BYTES: u64 = 5 * 1024 * 1024;

fn read_image(path: &str) -> Result<Vec<u8>, CoreError> {
    let p = Path::new(path);
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg"
    ) {
        return Err(CoreError::Invalid(
            "choose a PNG, JPEG, GIF, WebP or SVG image".into(),
        ));
    }
    let size = std::fs::metadata(p)
        .map_err(|e| CoreError::Invalid(format!("couldn't read {path}: {e}")))?
        .len();
    if size > MAX_IMAGE_BYTES {
        return Err(CoreError::Invalid(format!(
            "the image is {:.1} MB; images on a page can be at most 5 MB",
            size as f64 / 1_048_576.0
        )));
    }
    std::fs::read(p).map_err(|e| CoreError::Invalid(format!("couldn't read {path}: {e}")))
}

/// Where new projects go: `$XDG_DOCUMENTS_DIR/charted`.
pub fn projects_dir() -> PathBuf {
    default_sample_dir()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

/// `$XDG_DOCUMENTS_DIR/charted/Harbor & Pine`, falling back to `~/Documents`.
pub fn default_sample_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let docs = std::env::var_os("XDG_DOCUMENTS_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            let dirs = std::fs::read_to_string(home.join(".config/user-dirs.dirs")).ok()?;
            let line = dirs.lines().find(|l| l.starts_with("XDG_DOCUMENTS_DIR="))?;
            let v = line
                .split_once('=')?
                .1
                .trim_matches('"')
                .replace("$HOME", &home.display().to_string());
            Some(PathBuf::from(v))
        })
        .unwrap_or_else(|| home.join("Documents"));
    docs.join("charted").join(crate::sample::SAMPLE_NAME)
}
