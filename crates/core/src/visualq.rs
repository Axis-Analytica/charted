//! Report queries: a visual's data, the values a filter card lists, and checking DAX.

use charted_compiler::visual::{
    self, Catalog, Compiled, OrderBy, OrderTarget, QueryFilter, VisualQuery,
};
use charted_engine::{Database, sql};
use charted_model::{DataType, FieldRef, Model, Projection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use ts_rs::TS;

use crate::dataview::{DistinctValue, DistinctValues};
use crate::session::CoreError;
use crate::types;

/// Column types of the loaded tables, read once per session from DuckDB.
#[derive(Default, Clone)]
pub struct Schema {
    types: HashMap<(String, String), DataType>,
}

impl Schema {
    pub fn read(db: &Database, model: &Model) -> Self {
        let mut types = HashMap::new();
        for t in &model.tables {
            if let Ok(cols) = db.describe(&sql::ident(&t.source)) {
                for c in cols {
                    types.insert(
                        (t.source.clone(), c.name),
                        types::from_duckdb(&c.duckdb_type),
                    );
                }
            }
        }
        Self { types }
    }
}

impl Catalog for Schema {
    fn physical_type(&self, source: &str, column: &str) -> Option<DataType> {
        self.types
            .get(&(source.to_string(), column.to_string()))
            .copied()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DaxCheck {
    pub diagnostics: Vec<charted_dax::Diagnostic>,
    /// The SQL a visual would run for this measure alone (for the curious).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sql: Option<String>,
}

fn compile_err(e: visual::CompileError) -> CoreError {
    CoreError::Invalid(e.to_string())
}

pub fn compile(model: &Model, schema: &Schema, q: &VisualQuery) -> Result<Compiled, CoreError> {
    visual::compile(model, schema, q).map_err(compile_err)
}

/// The SQL listing a column's values for a filter card: ascending, under `filters`.
pub fn field_values_sql(
    model: &Model,
    schema: &Schema,
    field: &Projection,
    filters: &[QueryFilter],
    search: Option<&str>,
    limit: u32,
) -> Result<String, CoreError> {
    if !matches!(field.field, FieldRef::Column { .. }) {
        return Err(CoreError::Invalid("only columns list values".into()));
    }
    let c = compile(
        model,
        schema,
        &VisualQuery {
            groups: vec![Projection {
                aggregation: None,
                ..field.clone()
            }],
            values: vec![],
            filters: filters.to_vec(),
            order: vec![OrderBy {
                target: OrderTarget::Group(0),
                descending: false,
            }],
            limit: None,
        },
    )?;
    let search = search
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            format!(
                " WHERE contains(lower(CAST(g0 AS VARCHAR)), lower({}))",
                sql::string(s)
            )
        })
        .unwrap_or_default();
    Ok(format!(
        "SELECT CAST(g0 AS VARCHAR) AS k, g0 AS v, count(*) OVER () AS total FROM ({}) AS q{search} LIMIT {}",
        c.sql,
        limit.clamp(1, 10_000)
    ))
}

pub fn run_field_values(db: &Database, sql_text: &str) -> Result<DistinctValues, CoreError> {
    let rows = db.json_rows(sql_text, None)?;
    let mut total = 0;
    let mut values = Vec::new();
    for r in rows {
        let mut o: serde_json::Map<String, Value> = serde_json::from_str(&r).unwrap_or_default();
        total = o.get("total").and_then(Value::as_u64).unwrap_or(0);
        values.push(DistinctValue {
            key: o.get("k").and_then(Value::as_str).map(str::to_string),
            value: o.remove("v").unwrap_or(Value::Null),
            count: 0,
        });
    }
    Ok(DistinctValues { values, total })
}

/// Parses and binds `expression` as a measure; with no errors, also compiles it.
pub fn check_dax(
    model: &Model,
    schema: &Schema,
    expression: &str,
    measure: Option<&str>,
) -> DaxCheck {
    let (bound, diagnostics) = charted_dax::check(model, expression);
    if bound.is_none() {
        return DaxCheck {
            diagnostics,
            sql: None,
        };
    }
    // Compile through a scratch copy of the model with the expression in place, which
    // also catches circular references.
    let mut scratch = model.clone();
    let probe = "charted-probe-measure";
    let home = scratch
        .tables
        .iter()
        .position(|t| measure.is_some_and(|m| t.measures.iter().any(|x| x.id == m)));
    let Some(ti) = home.or(if scratch.tables.is_empty() {
        None
    } else {
        Some(0)
    }) else {
        return DaxCheck {
            diagnostics,
            sql: None,
        };
    };
    let id = match measure.and_then(|m| scratch.tables[ti].measure_mut(m)) {
        Some(m) => {
            m.expression = expression.to_string();
            m.id.clone()
        }
        None => {
            scratch.tables[ti].measures.push(charted_model::Measure {
                id: probe.into(),
                name: probe.into(),
                expression: expression.into(),
                language: Default::default(),
                description: None,
                format_string: None,
                display_folder: None,
                hidden: false,
                extras: Default::default(),
            });
            probe.into()
        }
    };
    let table = scratch.tables[ti].id.clone();
    match visual::compile(
        &scratch,
        schema,
        &VisualQuery {
            values: vec![Projection {
                field: FieldRef::Measure { table, measure: id },
                aggregation: None,
                display_name: None,
            }],
            ..Default::default()
        },
    ) {
        Ok(c) => DaxCheck {
            diagnostics,
            sql: Some(c.sql),
        },
        Err(e) => DaxCheck {
            diagnostics: vec![charted_dax::Diagnostic::error(
                charted_dax::Span {
                    start: 0,
                    end: expression.len(),
                },
                e.to_string(),
            )],
            sql: None,
        },
    }
}

/// The query behind a report visual: its fields by role (columns group, measures and
/// summarised columns are values), with report, page and visual filters, the visual's
/// sort or a sensible default for its type, and a row limit. With `totals`, the same
/// values without grouping (a table's total row).
pub fn report_visual_query(
    model: &Model,
    visual: &str,
    totals: bool,
    extra: &[QueryFilter],
) -> Result<(VisualQuery, Vec<String>), CoreError> {
    let (page, v) = model
        .report
        .visual(visual)
        .ok_or_else(|| CoreError::Invalid(format!("there is no visual with id {visual}")))?;
    let mut q = VisualQuery::default();
    // Output column names, groups first then values, as the compiler orders them.
    let mut group_names = Vec::new();
    let mut value_names = Vec::new();
    let mut slot: HashMap<(String, usize), OrderTarget> = HashMap::new();
    for (role, ps) in &v.roles {
        for (i, p) in ps.iter().enumerate() {
            let is_group = matches!(p.field, FieldRef::Column { .. }) && p.aggregation.is_none();
            if is_group {
                if totals {
                    continue;
                }
                slot.insert((role.clone(), i), OrderTarget::Group(q.groups.len()));
                q.groups.push(p.clone());
                group_names.push(format!("{role}:{i}"));
            } else {
                slot.insert((role.clone(), i), OrderTarget::Value(q.values.len()));
                q.values.push(p.clone());
                value_names.push(format!("{role}:{i}"));
            }
        }
    }
    let filters = model
        .report
        .filters
        .iter()
        .chain(&page.settings.filters)
        .chain(&v.filters)
        .filter(|f| f.is_active())
        .map(|f| QueryFilter {
            field: f.field.clone(),
            filter: f.filter.clone(),
        });
    q.filters = filters.chain(extra.iter().cloned()).collect();
    if let Some(s) = &v.sort
        && let Some(t) = slot.get(&(s.role.clone(), s.index))
    {
        q.order.push(OrderBy {
            target: *t,
            descending: s.descending,
        });
    } else if !q.groups.is_empty() && !q.values.is_empty() {
        // Bar and column charts rank categories by value unless the axis has an order
        // of its own (numbers, dates, a sort-by column).
        let ranked = matches!(v.visual_type.as_str(), "columnChart" | "barChart");
        let natural = match &q.groups[0].field {
            FieldRef::Column { table, column } => model
                .table(table)
                .and_then(|t| t.column(column))
                .is_some_and(|c| {
                    c.sort_by.is_some() || c.data_type.is_numeric() || c.data_type.is_temporal()
                }),
            _ => true,
        };
        if ranked && !natural {
            q.order.push(OrderBy {
                target: OrderTarget::Value(0),
                descending: true,
            });
        }
    }
    q.limit = Some(match v.visual_type.as_str() {
        "card" => 1,
        "lineChart" => 10_000,
        "table" => 5_000,
        _ => 1_000,
    });
    group_names.extend(value_names);
    Ok((q, group_names))
}

/// Renames the compiler's `g0…`/`v0…` columns to the visual's `role:index` names.
pub fn rename_columns(sql_text: &str, groups: usize, names: &[String]) -> String {
    let cols: Vec<String> = names
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let src = if i < groups {
                format!("g{i}")
            } else {
                format!("v{}", i - groups)
            };
            format!("{src} AS {}", sql::ident(n))
        })
        .collect();
    if cols.is_empty() {
        return "SELECT 1 WHERE FALSE".into();
    }
    format!("SELECT {} FROM ({sql_text}) AS visual", cols.join(", "))
}

/// A projection's name as shown in a visual: its display name, the field's name, or
/// "Sum of Quantity" for a summarised column.
pub fn projection_name(model: &Model, p: &Projection) -> String {
    if let Some(n) = &p.display_name {
        return n.clone();
    }
    let field = match &p.field {
        FieldRef::Column { table, column } => model
            .table(table)
            .and_then(|t| t.column(column))
            .map(|c| c.name.clone()),
        FieldRef::Measure { table, measure } => model
            .table(table)
            .and_then(|t| t.measures.iter().find(|m| &m.id == measure))
            .map(|m| m.name.clone()),
    }
    .unwrap_or_else(|| "(missing field)".into());
    match p.aggregation {
        None => field,
        Some(a) => {
            let what = match a {
                charted_model::Aggregation::Sum => "Sum",
                charted_model::Aggregation::Average => "Average",
                charted_model::Aggregation::Min => "Min",
                charted_model::Aggregation::Max => "Max",
                charted_model::Aggregation::Count => "Count",
                charted_model::Aggregation::DistinctCount => "Count (distinct)",
            };
            format!("{what} of {field}")
        }
    }
}

/// A visual's title: its own, or "Values by Groups" from its fields.
pub fn visual_title(model: &Model, v: &charted_model::Visual) -> String {
    if let Some(t) = v
        .objects
        .get("title")
        .and_then(|o| o.get("text"))
        .and_then(Value::as_str)
        .filter(|t| !t.trim().is_empty())
    {
        return t.to_string();
    }
    let mut groups = Vec::new();
    let mut values = Vec::new();
    for (_, p) in v.projections() {
        let name = projection_name(model, p);
        if matches!(p.field, FieldRef::Column { .. }) && p.aggregation.is_none() {
            groups.push(name);
        } else {
            values.push(name);
        }
    }
    match (values.is_empty(), groups.is_empty()) {
        (false, false) => format!("{} by {}", values.join(" and "), groups.join(" and ")),
        (false, true) => values.join(" and "),
        (true, false) => groups.join(" and "),
        (true, true) => String::new(),
    }
}
