//! The request/response API. The desktop shell, the command line and the development
//! WebSocket server all call `App::handle`, so they can never drift apart.

use charted_model::Change;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use ts_rs::TS;

use crate::recent::{self, RecentProject};
use crate::session::{CoreError, ProjectSnapshot, Session};

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

/// Requests for binary data (Arrow IPC), kept separate so large results skip JSON.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum DataRequest {
    Preview { table: String, limit: u32 },
}

pub struct App {
    session: Option<Session>,
    state_dir: PathBuf,
}

impl App {
    /// `state_dir` holds per-user state such as the recent-projects list.
    pub fn new(state_dir: PathBuf) -> Self {
        Self {
            session: None,
            state_dir,
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
        }
        Ok(())
    }

    pub fn data(&self, request: DataRequest) -> Result<Vec<u8>, ApiError> {
        let s = self.session.as_ref().ok_or(CoreError::NoProject)?;
        match request {
            DataRequest::Preview { table, limit } => Ok(s.preview(&table, limit.min(10_000))?),
        }
    }
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
