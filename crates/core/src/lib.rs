//! charted's application core.
//!
//! Owns the open project (model, undo history, loaded data), saving and crash recovery,
//! the request API shared by the desktop shell, the command line and the test server,
//! and integration with the Omarchy desktop.

pub mod api;
mod atomic;
pub mod bundle;
pub mod check;
pub mod crash;
pub mod dataview;
pub mod env;
pub mod ids;
pub mod interop;
pub mod pbimport;
pub mod project;
pub mod queryedit;
pub mod recent;
pub mod relcheck;
pub mod reportfile;
pub mod reporttheme;
pub mod sample;
pub mod session;
pub mod sources;
mod types;
pub mod visualq;

pub use api::{ApiError, App, AppState, DataRequest, Request};
pub use session::{CoreError, ProjectSnapshot, Session};
