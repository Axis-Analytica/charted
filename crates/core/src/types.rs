//! DuckDB type names to model data types.

use charted_model::DataType;

pub fn from_duckdb(t: &str) -> DataType {
    let u = t.to_ascii_uppercase();
    let base = u.split('(').next().unwrap_or("").trim();
    match base {
        "VARCHAR" | "TEXT" | "STRING" | "CHAR" | "BPCHAR" | "UUID" | "ENUM" => DataType::String,
        "BIGINT" | "INTEGER" | "SMALLINT" | "TINYINT" | "HUGEINT" | "UBIGINT" | "UINTEGER"
        | "USMALLINT" | "UTINYINT" | "UHUGEINT" | "INT" | "INT8" | "INT4" => DataType::Int64,
        "DOUBLE" | "FLOAT" | "REAL" => DataType::Double,
        "DECIMAL" | "NUMERIC" => DataType::Decimal,
        "BOOLEAN" | "BOOL" => DataType::Boolean,
        "DATE" => DataType::Date,
        "TIME" => DataType::Time,
        "TIMESTAMP" | "TIMESTAMP_S" | "TIMESTAMP_MS" | "TIMESTAMP_NS" | "DATETIME" => {
            DataType::DateTime
        }
        "TIMESTAMP WITH TIME ZONE" | "TIMESTAMPTZ" => DataType::DateTimeTz,
        "BLOB" | "BYTEA" => DataType::Binary,
        _ if u.starts_with("TIMESTAMP WITH TIME ZONE") => DataType::DateTimeTz,
        _ => DataType::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_types() {
        assert_eq!(from_duckdb("DECIMAL(12,2)"), DataType::Decimal);
        assert_eq!(from_duckdb("BIGINT"), DataType::Int64);
        assert_eq!(
            from_duckdb("TIMESTAMP WITH TIME ZONE"),
            DataType::DateTimeTz
        );
        assert_eq!(from_duckdb("STRUCT(a INTEGER)"), DataType::Unknown);
    }
}
