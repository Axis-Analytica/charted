//! Apache Ossie semantic model I/O for charted.
//!
//! charted stores its semantic model as an Ossie YAML document (`model.ossie.yaml`), so a
//! model built in charted can be read by other Ossie tools and converted to a Power BI
//! model, and models from those tools can be opened in charted.

pub mod document;
mod map;
pub mod stash;
#[cfg(feature = "schema")]
pub mod validate;

pub use document::{Document, SPEC_VERSION};
pub use map::{
    ANSI_SQL, DAX, GeneratedDialects, MapError, OSSIE_SQL, Warning, from_document, to_document,
};

use charted_model::Model;

#[derive(Debug, thiserror::Error)]
pub enum OssieError {
    #[error("not a valid Apache Ossie YAML document: {0}")]
    Yaml(#[from] serde_yaml_ng::Error),
    #[error(transparent)]
    Map(#[from] MapError),
}

/// Parses an Ossie YAML (or JSON) document into a model.
pub fn read_str(text: &str) -> Result<(Model, Vec<Warning>), OssieError> {
    let doc: Document = serde_yaml_ng::from_str(text)?;
    Ok(from_document(&doc)?)
}

/// Parses YAML into a JSON value (for schema validation).
pub fn yaml_to_json(text: &str) -> Result<serde_json::Value, serde_yaml_ng::Error> {
    serde_yaml_ng::from_str(text)
}

/// Serialises `model` as an Ossie YAML document.
pub fn write_string(model: &Model, generated: &GeneratedDialects) -> String {
    let doc = to_document(model, generated);
    let body = serde_yaml_ng::to_string(&doc).expect("an Ossie document always serialises");
    format!("# Apache Ossie semantic model, written by charted. https://ossie.apache.org/\n{body}")
}
