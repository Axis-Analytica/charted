//! Compiles charted's declarative structures to DuckDB SQL text.
//!
//! This crate never touches a database or the file system, so it also builds for
//! WebAssembly (the editor uses it to preview generated SQL).

pub mod graph;
pub mod interp;
pub mod mformula;
pub mod ossie;
pub mod quote;
pub mod steps;
pub mod visual;

use charted_model::DataType;

/// The physical column a calculated column's values are stored in, next to the loaded
/// columns of its table.
pub fn calc_column_name(id: &str) -> String {
    format!("__calc_{id}")
}

/// DuckDB's name for a model data type.
pub fn sqltype(t: DataType) -> Option<&'static str> {
    Some(match t {
        DataType::String => "VARCHAR",
        DataType::Int64 => "BIGINT",
        DataType::Double => "DOUBLE",
        DataType::Decimal => "DECIMAL(19,4)",
        DataType::Boolean => "BOOLEAN",
        DataType::Date => "DATE",
        DataType::Time => "TIME",
        DataType::DateTime => "TIMESTAMP",
        DataType::DateTimeTz => "TIMESTAMPTZ",
        DataType::Binary => "BLOB",
        DataType::Unknown => return None,
    })
}
