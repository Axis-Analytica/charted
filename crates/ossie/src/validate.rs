//! Validation against the vendored Apache Ossie JSON Schema (see schema/UPSTREAM).

use std::sync::OnceLock;

pub const SCHEMA: &str = include_str!("../schema/ossie-schema.json");

fn validator() -> &'static jsonschema::Validator {
    static V: OnceLock<jsonschema::Validator> = OnceLock::new();
    V.get_or_init(|| {
        let schema: serde_json::Value =
            serde_json::from_str(SCHEMA).expect("vendored schema is valid JSON");
        jsonschema::validator_for(&schema).expect("vendored schema compiles")
    })
}

/// Returns schema violations as human-readable strings (empty when valid).
pub fn validate(document: &serde_json::Value) -> Vec<String> {
    validator()
        .iter_errors(document)
        .map(|e| format!("{} at {}", e, e.instance_path()))
        .collect()
}
