//! SQL quoting. Every identifier and literal that reaches generated SQL goes through here.

pub fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub fn string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}
