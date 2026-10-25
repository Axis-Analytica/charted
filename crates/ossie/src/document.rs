//! Serde types mirroring the Apache Ossie document structure. Field order follows the
//! specification so written YAML reads naturally and diffs cleanly.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const SPEC_VERSION: &str = "0.2.0.dev0";

/// Keys this version of charted does not know, preserved verbatim.
pub type Unknown = BTreeMap<String, Value>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub version: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_context: Option<Value>,
    pub datasets: Vec<Dataset>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relationships: Vec<Relationship>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metrics: Vec<Metric>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_extensions: Vec<CustomExtension>,
    #[serde(flatten)]
    pub unknown: Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dataset {
    pub name: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub primary_key: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unique_keys: Vec<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_context: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<Field>,
    /// Dataset-scoped metrics (spec addition, Oct 2026). charted writes root-level metrics
    /// for converter compatibility but reads both.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metrics: Vec<Metric>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_extensions: Vec<CustomExtension>,
    #[serde(flatten)]
    pub unknown: Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub expression: Expression,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimension: Option<Dimension>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub datatype: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_context: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_extensions: Vec<CustomExtension>,
    #[serde(flatten)]
    pub unknown: Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dimension {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_time: Option<bool>,
    #[serde(flatten)]
    pub unknown: Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metric {
    pub name: String,
    pub expression: Expression,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub datatype: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_context: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_extensions: Vec<CustomExtension>,
    #[serde(flatten)]
    pub unknown: Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Relationship {
    pub name: String,
    pub from: String,
    pub to: String,
    pub from_columns: Vec<String>,
    pub to_columns: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_context: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_extensions: Vec<CustomExtension>,
    #[serde(flatten)]
    pub unknown: Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Expression {
    pub dialects: Vec<DialectExpression>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DialectExpression {
    pub dialect: String,
    pub expression: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomExtension {
    pub vendor_name: String,
    /// A JSON document encoded as a string, per the specification.
    pub data: String,
}

impl Expression {
    pub fn single(dialect: &str, expression: impl Into<String>) -> Self {
        Self {
            dialects: vec![DialectExpression {
                dialect: dialect.into(),
                expression: expression.into(),
            }],
        }
    }

    pub fn get(&self, dialect: &str) -> Option<&str> {
        self.dialects
            .iter()
            .find(|d| d.dialect == dialect)
            .map(|d| d.expression.as_str())
    }
}
