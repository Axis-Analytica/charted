//! Data-loading queries: a source plus a list of applied steps (Power Query's model),
//! compiled into a single SELECT that DuckDB optimises as a whole, so filters and column
//! choices are pushed down into the file readers.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::quote;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Query {
    pub id: String,
    /// Display name, shown in the query editor.
    pub name: String,
    /// The DuckDB table the query loads into (the model table's `source`).
    pub table: String,
    pub source: Source,
    #[serde(default)]
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum Source {
    Csv {
        /// Path relative to the project folder, or absolute.
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        delimiter: Option<String>,
        #[serde(default = "yes")]
        header: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encoding: Option<String>,
    },
    Parquet {
        path: String,
    },
    Json {
        path: String,
    },
}

fn yes() -> bool {
    true
}

/// One applied step. More arrive with the query editor (milestone 5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum Step {
    SelectColumns { columns: Vec<String> },
    RemoveColumns { columns: Vec<String> },
    RenameColumns { renames: Vec<(String, String)> },
}

impl Source {
    pub fn path(&self) -> &str {
        match self {
            Source::Csv { path, .. } | Source::Parquet { path } | Source::Json { path } => path,
        }
    }
}

/// Resolves a source path against the project folder (both as plain strings).
pub fn resolve_path(project_dir: &str, path: &str) -> String {
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("{}/{}", project_dir.trim_end_matches('/'), path)
    }
}

fn source_sql(source: &Source, project_dir: &str) -> String {
    let path = quote::string(&resolve_path(project_dir, source.path()));
    match source {
        Source::Csv {
            delimiter,
            header,
            encoding,
            ..
        } => {
            let mut opts = vec![format!("header = {header}")];
            if let Some(d) = delimiter {
                opts.push(format!("delim = {}", quote::string(d)));
            }
            if let Some(e) = encoding {
                opts.push(format!("encoding = {}", quote::string(e)));
            }
            format!("read_csv({path}, {})", opts.join(", "))
        }
        Source::Parquet { .. } => format!("read_parquet({path})"),
        Source::Json { .. } => format!("read_json_auto({path})"),
    }
}

fn step_sql(step: &Step, input: &str) -> String {
    match step {
        Step::SelectColumns { columns } => {
            let cols: Vec<String> = columns.iter().map(|c| quote::ident(c)).collect();
            format!("SELECT {} FROM {input}", cols.join(", "))
        }
        Step::RemoveColumns { columns } => {
            let cols: Vec<String> = columns.iter().map(|c| quote::ident(c)).collect();
            format!("SELECT * EXCLUDE ({}) FROM {input}", cols.join(", "))
        }
        Step::RenameColumns { renames } => {
            let cols: Vec<String> = renames
                .iter()
                .map(|(from, to)| format!("{} AS {}", quote::ident(from), quote::ident(to)))
                .collect();
            format!("SELECT * RENAME ({}) FROM {input}", cols.join(", "))
        }
    }
}

/// Compiles a query to one SELECT statement, with each step as a CTE.
pub fn to_sql(query: &Query, project_dir: &str) -> String {
    let source = source_sql(&query.source, project_dir);
    if query.steps.is_empty() {
        return format!("SELECT * FROM {source}");
    }
    let mut ctes = vec![format!("s0 AS (SELECT * FROM {source})")];
    for (i, step) in query.steps.iter().enumerate() {
        ctes.push(format!(
            "s{} AS ({})",
            i + 1,
            step_sql(step, &format!("s{i}"))
        ));
    }
    format!(
        "WITH {}\nSELECT * FROM s{}",
        ctes.join(",\n"),
        query.steps.len()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_sources_and_steps() {
        let q = Query {
            id: "q1".into(),
            name: "Stores".into(),
            table: "store".into(),
            source: Source::Csv {
                path: "data/it's.csv".into(),
                delimiter: Some(";".into()),
                header: true,
                encoding: None,
            },
            steps: vec![
                Step::RemoveColumns {
                    columns: vec!["junk".into()],
                },
                Step::RenameColumns {
                    renames: vec![("City Name".into(), "City".into())],
                },
            ],
        };
        insta::assert_snapshot!(to_sql(&q, "/home/me/Sales"), @r#"
        WITH s0 AS (SELECT * FROM read_csv('/home/me/Sales/data/it''s.csv', header = true, delim = ';')),
        s1 AS (SELECT * EXCLUDE ("junk") FROM s0),
        s2 AS (SELECT * RENAME ("City Name" AS "City") FROM s1)
        SELECT * FROM s2
        "#);
        let p = Query {
            steps: vec![],
            source: Source::Parquet {
                path: "/abs/sales.parquet".into(),
            },
            ..q
        };
        assert_eq!(
            to_sql(&p, "/x"),
            "SELECT * FROM read_parquet('/abs/sales.parquet')"
        );
    }
}
