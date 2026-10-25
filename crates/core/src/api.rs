//! The request/response API. The desktop shell, the command line and the development
//! WebSocket server all call `App::handle`, so they can never drift apart.

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
}

impl From<CoreError> for ApiError {
    fn from(e: CoreError) -> Self {
        Self {
            code: e.code().into(),
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
    View(Box<View>, DataRequest),
    Arrow(Database, String),
    /// A query that a later one with the same tag cancels.
    Tagged(Database, String, Running, String),
    FieldValues(Database, String),
    Relationship(Database, String),
    Ready(Vec<u8>),
}

impl DataJob {
    pub fn run(self) -> Result<Vec<u8>, ApiError> {
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
                        }
                    } else {
                        CoreError::from(e).into()
                    }
                });
            }
            DataJob::FieldValues(db, sql) => return json(visualq::run_field_values(&db, &sql)),
            DataJob::Relationship(db, sql) => return json(crate::relcheck::run(&db, &sql)),
            DataJob::Ready(bytes) => return Ok(bytes),
        };
        let v = &*v;
        match request {
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
            running: Running::default(),
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
        let session = Session::open(path)?;
        recent::remember(
            &self.state_dir,
            &session.project.manifest.name,
            &session.project.manifest_path,
        );
        self.session = Some(session);
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
            Request::OpenProject { path } => self.open(Path::new(&path))?,
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
        Ok(DataJob::View(Box::new(view), request))
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
