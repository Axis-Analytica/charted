//! DuckDB-backed execution engine.
//!
//! Query results cross into the UI as Arrow IPC stream bytes, which the
//! webview decodes without a JSON step.

mod arrow;

pub use arrow::{query_arrow_ipc, ArrowError};
