//! Vendor extension blobs (`custom_extensions` entries).
//!
//! The `POWER_BI` blob follows the convention of the official Apache Ossie Power BI
//! converter (`apache/ossie-converters`, `microsoft/`): a JSON object with `"_v": 2` and
//! TMSL property names, serialised the way Python's `json.dumps(..., sort_keys=True)`
//! does. Matching it byte-for-byte keeps files written by charted and by the converter
//! interchangeable and their diffs quiet.

use serde_json::{Map, Value};

use crate::document::CustomExtension;

pub const POWER_BI: &str = "POWER_BI";
pub const CHARTED: &str = "CHARTED";
pub const POWER_BI_STASH_VERSION: i64 = 2;
pub const CHARTED_STASH_VERSION: i64 = 1;

#[derive(Debug, thiserror::Error)]
pub enum StashError {
    #[error("{vendor} custom_extensions data is not valid JSON: {source}")]
    Json { vendor: String, source: serde_json::Error },
    #[error("{vendor} custom_extensions data must be a JSON object")]
    NotObject { vendor: String },
    #[error("{vendor} custom_extensions data was written by a newer version (_v {found}, this charted understands {supported})")]
    TooNew { vendor: String, found: i64, supported: i64 },
}

/// Reads the blob for `vendor`, without its `_v` key. Missing blobs read as empty.
pub fn read(exts: &[CustomExtension], vendor: &str, supported: i64) -> Result<Map<String, Value>, StashError> {
    let Some(ext) = exts.iter().find(|e| e.vendor_name == vendor) else { return Ok(Map::new()) };
    let value: Value = serde_json::from_str(&ext.data).map_err(|source| StashError::Json { vendor: vendor.into(), source })?;
    let Value::Object(mut map) = value else { return Err(StashError::NotObject { vendor: vendor.into() }) };
    let version = map.remove("_v").and_then(|v| v.as_i64()).unwrap_or(supported);
    if version > supported {
        return Err(StashError::TooNew { vendor: vendor.into(), found: version, supported });
    }
    Ok(map)
}

/// Writes (or replaces) the blob for `vendor`. An empty blob writes nothing, so models
/// without vendor-specific properties stay vendor-neutral.
pub fn write(exts: &mut Vec<CustomExtension>, vendor: &str, version: i64, data: Map<String, Value>) {
    exts.retain(|e| e.vendor_name != vendor);
    if data.is_empty() {
        return;
    }
    let mut payload = Map::new();
    payload.insert("_v".into(), Value::from(version));
    payload.extend(data);
    exts.push(CustomExtension { vendor_name: vendor.into(), data: python_json(&Value::Object(payload)) });
}

/// Extensions of vendors charted does not interpret.
pub fn foreign(exts: &[CustomExtension]) -> Vec<CustomExtension> {
    exts.iter().filter(|e| e.vendor_name != POWER_BI && e.vendor_name != CHARTED).cloned().collect()
}

/// Serialises like Python's `json.dumps(value, sort_keys=True)`: keys sorted, `", "` and
/// `": "` separators, non-ASCII escaped as `\uXXXX`.
pub fn python_json(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => write_string(out, s),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_string(out, k);
                out.push_str(": ");
                write_value(out, &map[k]);
            }
            out.push('}');
        }
    }
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn matches_python_json_dumps_sort_keys() {
        // Expected strings produced by CPython 3: json.dumps(v, sort_keys=True)
        let v = json!({"_v": 2, "isHidden": true, "formatString": "#,0.00", "table": "Sales"});
        assert_eq!(python_json(&v), r##"{"_v": 2, "formatString": "#,0.00", "isHidden": true, "table": "Sales"}"##);
        let v = json!({"name": "Café ☕", "list": [1, "a\"b", null]});
        assert_eq!(python_json(&v), r#"{"list": [1, "a\"b", null], "name": "Caf\u00e9 \u2615"}"#);
    }
}
