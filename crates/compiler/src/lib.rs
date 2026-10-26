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

use charted_model::{DataType, Model};

/// Whether a filter on these columns of a table replaces every filter on the table, as
/// DAX does for the date column of a marked date table and for date columns used in
/// relationships (an empty list stands for every column).
pub fn replaces_date_table(model: &Model, table: &str, columns: &[String]) -> bool {
    let Some(t) = model.table(table) else {
        return false;
    };
    t.columns
        .iter()
        .filter(|c| columns.is_empty() || columns.contains(&c.id))
        .any(|c| {
            matches!(
                c.data_type,
                DataType::Date | DataType::DateTime | DataType::DateTimeTz
            ) && (t.is_date_table
                || model
                    .relationships
                    .iter()
                    .any(|r| r.from_column == c.id || r.to_column == c.id))
        })
}

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
