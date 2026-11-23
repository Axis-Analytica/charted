//! Compiles charted's declarative structures to DuckDB SQL text.
//!
//! This crate never touches a database or the file system, so it also builds for
//! WebAssembly (the editor uses it to preview generated SQL).

pub mod clock;
pub mod graph;
pub mod interp;
pub mod mformula;
pub mod ossie;
pub mod quote;
pub mod steps;
pub mod visual;

use charted_model::{DataType, Model};

/// Whether `ALLSELECTED` of `requested` columns of `table` still sees a filter on
/// `filter_columns` of `filter_table`.
///
/// Power BI ignores a visual filter table on another column of a marked date table
/// for `ALLSELECTED` of its date column (`ALLSELECTED ( Date[Date] )` under a
/// filter table on `Date[Year]`), and keeps that filter for `ALLSELECTED` of the
/// whole table. A filter on the requested columns themselves stays, as does a
/// filter the expression added (`Date[Year] = 2025` inside `CALCULATE`). A date
/// column that is only in a relationship, on a table that is not a date table,
/// keeps every filter. `from_query` marks that visual filter table. `requested`
/// empty means the whole table.
pub fn allselected_keeps_filter(
    model: &Model,
    table: &str,
    requested: &[String],
    filter_table: &str,
    filter_columns: &[String],
    from_query: bool,
) -> bool {
    if !from_query || filter_table != table || requested.is_empty() || filter_columns.is_empty() {
        return true;
    }
    if !model.table(table).is_some_and(|t| t.is_date_table) {
        return true;
    }
    if !replaces_date_table(model, table, requested) {
        return true;
    }
    filter_columns
        .iter()
        .any(|c| requested.iter().any(|r| r == c))
}

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
