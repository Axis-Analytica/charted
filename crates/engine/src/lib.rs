//! DuckDB-backed execution engine.
//!
//! Query results cross into the UI as Arrow IPC stream bytes, which the webview decodes
//! without a JSON step. Nothing here downloads DuckDB extensions on its own: automatic
//! installation is disabled, and network-backed sources are opt-in elsewhere.

mod arrow;
mod db;
pub mod sql;

pub use arrow::{ArrowError, query_arrow_ipc};
pub use db::{ColumnInfo, Database, DbError, DbOptions, InterruptHandle};
