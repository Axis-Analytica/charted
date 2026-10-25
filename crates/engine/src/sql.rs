//! Helpers for building SQL text safely. Identifiers are always quoted and literals
//! always escaped; user input never reaches SQL any other way.

/// Quotes an identifier for DuckDB: `my "col"` becomes `"my ""col"""`.
pub fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Quotes a string literal: `it's` becomes `'it''s'`.
pub fn string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_cannot_be_escaped() {
        assert_eq!(ident("a\"b"), "\"a\"\"b\"");
        assert_eq!(string("x'; DROP TABLE t; --"), "'x''; DROP TABLE t; --'");
    }
}
