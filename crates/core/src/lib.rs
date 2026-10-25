//! charted's application core.
//!
//! Owns the open project (model, undo history, loaded data), saving and crash recovery,
//! the request API shared by the desktop shell, the command line and the test server,
//! and integration with the Omarchy desktop.

pub mod api;
mod atomic;
pub mod check;
pub mod dataview;
pub mod env;
pub mod ids;
pub mod project;
pub mod recent;
pub mod sample;
pub mod session;
mod types;

pub use api::{ApiError, App, AppState, DataRequest, Request};
pub use session::{CoreError, ProjectSnapshot, Session};
