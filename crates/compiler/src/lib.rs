//! Compiles charted's declarative structures to DuckDB SQL text.
//!
//! This crate never touches a database or the file system, so it also builds for
//! WebAssembly (the editor uses it to preview generated SQL).

pub mod quote;
pub mod steps;
