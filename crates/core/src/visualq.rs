//! Report queries: a visual's data, the values a filter card lists, and checking DAX.

use charted_compiler::visual::{
    self, Catalog, Compiled, OrderBy, OrderTarget, QueryFilter, RowFilter, VisualQuery,
};
use charted_engine::{Database, sql};
use charted_model::{DataType, FieldRef, FilterKind, InteractionKind, Model, Page, Projection};
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

/// The SQL listing a column's values for a filter card: ascending, under `filters`,
/// and only the rows `security` (the roles viewed as) lets through.
pub fn field_values_sql(
    model: &Model,
    schema: &Schema,
    field: &Projection,
    filters: &[QueryFilter],
    security: &[RowFilter],
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
            keys: false,
            security: security.to_vec(),
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
/// summarised columns are values), with report, page and visual filters, the choices
/// of the page's slicers (where the page's interactions let them through), the
/// visual's sort or a sensible default for its type, and a row limit. With `totals`,
/// the same values without grouping (a table's total row). `extra` filters are added
/// (a selection in another visual).
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
    // A drilled hierarchy shows some of its levels, filtered to the members drilled into.
    let drill = charted_model::drill_role(&v.visual_type)
        .and_then(|r| Some((r, v.roles.get(r)?)))
        .filter(|(_, ps)| ps.len() > 1);
    let mut drill_filters = Vec::new();
    let shown = |role: &str, i: usize| match drill {
        Some((r, ps)) if r == role => {
            let (start, depth, _) = v.drill_levels(ps.len());
            (start..=depth).contains(&i)
        }
        _ => true,
    };
    if let Some((_, ps)) = drill {
        for (level, key) in v.drill_levels(ps.len()).2 {
            drill_filters.push(QueryFilter {
                field: Projection {
                    aggregation: None,
                    ..ps[level].clone()
                },
                filter: FilterKind::Basic {
                    values: vec![key],
                    exclude: false,
                },
            });
        }
    }
    for (role, ps) in &v.roles {
        for (i, p) in ps.iter().enumerate() {
            let is_group = matches!(p.field, FieldRef::Column { .. }) && p.aggregation.is_none();
            if is_group && !shown(role, i) {
                continue;
            }
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
    q.filters = filters
        .chain(slicer_filters(page, &v.id))
        .chain(drill_filters)
        .chain(extra.iter().cloned())
        .collect();
    if let Some(s) = &v.sort
        && let Some(t) = slot.get(&(s.role.clone(), s.index))
    {
        q.order.push(OrderBy {
            target: *t,
            descending: s.descending,
        });
    } else if !q.groups.is_empty() && !q.values.is_empty() {
        // Bar, column and pie charts rank categories by value unless the axis has an
        // order of its own (numbers, dates, a sort-by column).
        let ranked = matches!(
            v.visual_type.as_str(),
            "columnChart"
                | "barChart"
                | "stackedColumnChart"
                | "stackedBarChart"
                | "hundredPercentStackedColumnChart"
                | "hundredPercentStackedBarChart"
                | "lineClusteredColumnComboChart"
                | "lineStackedColumnComboChart"
                | "funnel"
                | "treemap"
                | "pieChart"
                | "donutChart"
        );
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
            // By the first of the values role (a combo chart's columns, not its line).
            let target = slot
                .get(&("values".to_string(), 0))
                .copied()
                .unwrap_or(OrderTarget::Value(0));
            q.order.push(OrderBy {
                target,
                descending: true,
            });
        }
    }
    let multiples = v
        .roles
        .get("smallMultiples")
        .is_some_and(|ps| !ps.is_empty());
    q.limit = Some(match v.visual_type.as_str() {
        "card" | "multiRowCard" | "gauge" => 1,
        _ if multiples => 10_000,
        "lineChart" | "scatterChart" | "kpi" => 10_000,
        "table" | "matrix" | "slicer" => 5_000,
        _ => 1_000,
    });
    group_names.extend(value_names);
    Ok((q, group_names))
}

/// The filters that the slicers on `page` put on `target`: each slicer's choice on
/// its field, unless the page turns that interaction off. A slicer never filters
/// itself.
pub fn slicer_filters(page: &Page, target: &str) -> Vec<QueryFilter> {
    page.visuals
        .iter()
        .filter(|s| s.is_slicer() && s.id != target)
        .filter(|s| page.interaction(&s.id, target) != InteractionKind::None)
        .filter_map(|s| {
            let field = s.roles.get("field")?.first()?;
            let filter = s.selection.clone()?;
            (!matches!(filter, FilterKind::All)).then(|| QueryFilter {
                field: Projection {
                    aggregation: None,
                    ..field.clone()
                },
                filter,
            })
        })
        .collect()
}

/// How a report visual's data is asked for.
#[derive(Debug, Clone, Default)]
pub struct VisualSqlOptions<'a> {
    /// Values without grouping (a table's total row).
    pub totals: bool,
    /// Added filters (a selection elsewhere that filters this visual).
    pub filters: &'a [QueryFilter],
    /// A selection elsewhere that highlights this visual: each value is also computed
    /// under these filters, as `hl:role:index`.
    pub highlight: &'a [QueryFilter],
    /// Name columns after the fields ("Sum of Quantity") instead of `role:index`, with
    /// no key columns (exports and the command line).
    pub captions: bool,
    /// A slicer's search: only values whose text contains this (any case), so a field
    /// with more values than a slicer lists can still be searched.
    pub search: Option<&'a str>,
    /// Row-level security: the filters of the roles the report is viewed as
    /// (`roles::security`). Every query behind the visual carries them.
    pub security: &'a [RowFilter],
}

/// The SQL behind a report visual. Columns are named `role:index`, with each group's
/// value as text in `key:role:index` (what a selection filters on). A matrix returns
/// every level of its row and column hierarchies (subtotals and totals), each row
/// marked with `level:rows`, `level:columns` and its position within its level
/// (`order`). A slicer lists its field's values, or for a range slicer the smallest and
/// largest (`min`, `max`).
pub fn report_visual_sql(
    model: &Model,
    schema: &Schema,
    visual: &str,
    o: &VisualSqlOptions,
) -> Result<String, CoreError> {
    let (_, v) = model
        .report
        .visual(visual)
        .ok_or_else(|| CoreError::Invalid(format!("there is no visual with id {visual}")))?;
    let (mut q, mut names) = report_visual_query(model, visual, o.totals, o.filters)?;
    q.security = o.security.to_vec();
    if v.is_slicer() {
        return slicer_sql(model, schema, v, q, o.search);
    }
    if o.captions {
        let names: Vec<String> = q
            .groups
            .iter()
            .chain(&q.values)
            .map(|p| projection_name(model, p))
            .collect();
        let c = compile(model, schema, &q)?;
        return Ok(select_as(
            &c.sql,
            &columns(q.groups.len(), names.len(), &names, false),
        ));
    }
    // Fields that only drive conditional formatting are hidden values.
    for (name, p) in formatting_values(v) {
        q.values.push(p);
        names.push(name);
    }
    if v.visual_type == "matrix" && !o.totals {
        let value_names = names[q.groups.len()..].to_vec();
        return matrix_sql(model, schema, v, q, &value_names);
    }
    q.keys = true;
    let out = visual_rows_sql(model, schema, &q, &names, o.highlight)?;
    combo_lines(model, schema, v, &q, &names, out)
}

/// The values that conditional formatting on a table or matrix reads from fields of
/// their own (`objects.conditionalFormatting[column][kind].field`), named
/// `cf:{column}:{kind}`.
pub fn formatting_values(v: &charted_model::Visual) -> Vec<(String, Projection)> {
    if !matches!(v.visual_type.as_str(), "table" | "matrix") {
        return Vec::new();
    }
    let Some(Value::Object(columns)) = v.objects.get("conditionalFormatting") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (column, kinds) in columns {
        let Some(kinds) = kinds.as_object() else {
            continue;
        };
        for (kind, rule) in kinds {
            if let Some(p) = rule
                .get("field")
                .filter(|f| !f.is_null())
                .and_then(|f| serde_json::from_value::<Projection>(f.clone()).ok())
            {
                out.push((format!("cf:{column}:{kind}"), p));
            }
        }
    }
    out
}

/// A combo chart with a legend: its line values belong to the category, not to each
/// legend value, so they come from a query without the legend, joined on the rest.
fn combo_lines(
    model: &Model,
    schema: &Schema,
    v: &charted_model::Visual,
    q: &VisualQuery,
    names: &[String],
    out: String,
) -> Result<String, CoreError> {
    let groups = q.groups.len();
    let combo = v.visual_type.starts_with("line") && v.visual_type.ends_with("ComboChart");
    let Some(series) = names[..groups].iter().position(|n| n == "series:0") else {
        return Ok(out);
    };
    let lines: Vec<(usize, &String)> = names[groups..]
        .iter()
        .enumerate()
        .filter(|(_, n)| n.starts_with("lineValues:"))
        .collect();
    if !combo || lines.is_empty() {
        return Ok(out);
    }
    let mut lq = q.clone();
    lq.groups.remove(series);
    lq.values = lines.iter().map(|(i, _)| q.values[*i].clone()).collect();
    lq.order.clear();
    lq.limit = None;
    lq.keys = false;
    let l = compile(model, schema, &lq)?;
    let others: Vec<&String> = names[..groups]
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != series)
        .map(|(_, n)| n)
        .collect();
    let on = if others.is_empty() {
        "TRUE".to_string()
    } else {
        others
            .iter()
            .enumerate()
            .map(|(j, n)| format!("a.{} IS NOT DISTINCT FROM l.g{j}", sql::ident(n)))
            .collect::<Vec<_>>()
            .join(" AND ")
    };
    let exclude: Vec<String> = lines.iter().map(|(_, n)| sql::ident(n)).collect();
    let picks: Vec<String> = lines
        .iter()
        .enumerate()
        .map(|(j, (_, n))| format!("l.v{j} AS {}", sql::ident(n)))
        .collect();
    Ok(format!(
        "SELECT a.* EXCLUDE (combo_rn, {}), {} FROM (SELECT *, row_number() OVER () AS combo_rn FROM ({out}) AS o) AS a LEFT JOIN ({}) AS l ON {on} ORDER BY a.combo_rn",
        exclude.join(", "),
        picks.join(", "),
        l.sql
    ))
}

/// A visual's rows (`role:index` columns with keys), with each value's highlighted
/// part (`hl:role:index`) when a selection elsewhere highlights it.
fn visual_rows_sql(
    model: &Model,
    schema: &Schema,
    q: &VisualQuery,
    names: &[String],
    highlight: &[QueryFilter],
) -> Result<String, CoreError> {
    let groups = q.groups.len();
    let cols = columns(groups, names.len(), names, true);
    let full = compile(model, schema, q)?;
    if highlight.is_empty() || q.values.is_empty() {
        return Ok(select_as(&full.sql, &cols));
    }
    // The highlight: the same values with the selection's filters added, joined to the
    // full result on the groups.
    let mut hq = q.clone();
    hq.filters.extend(highlight.iter().cloned());
    hq.order.clear();
    hq.limit = None;
    hq.keys = false;
    let hl = compile(model, schema, &hq)?;
    let on = if groups == 0 {
        "TRUE".to_string()
    } else {
        (0..groups)
            .map(|i| format!("a.g{i} IS NOT DISTINCT FROM b.g{i}"))
            .collect::<Vec<_>>()
            .join(" AND ")
    };
    let mut picks: Vec<String> = cols
        .iter()
        .map(|(src, name)| format!("a.{src} AS {}", sql::ident(name)))
        .collect();
    for (i, name) in names.iter().enumerate().skip(groups) {
        picks.push(format!(
            "b.v{} AS {}",
            i - groups,
            sql::ident(&format!("hl:{name}"))
        ));
    }
    Ok(format!(
        "SELECT {} FROM (SELECT *, row_number() OVER () AS rn FROM ({}) AS f) AS a LEFT JOIN ({}) AS b ON {on} ORDER BY a.rn",
        picks.join(", "),
        full.sql,
        hl.sql
    ))
}

/// Compiler columns (`g0…`, `v0…`, `k0…`) paired with the names to give them.
fn columns(groups: usize, total: usize, names: &[String], keys: bool) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = (0..total)
        .map(|i| {
            let src = if i < groups {
                format!("g{i}")
            } else {
                format!("v{}", i - groups)
            };
            (src, names[i].clone())
        })
        .collect();
    if keys {
        for (i, name) in names.iter().take(groups).enumerate() {
            out.push((format!("k{i}"), format!("key:{name}")));
        }
    }
    out
}

fn select_as(sql_text: &str, cols: &[(String, String)]) -> String {
    if cols.is_empty() {
        return "SELECT 1 WHERE FALSE".into();
    }
    format!(
        "SELECT {} FROM ({sql_text}) AS visual",
        cols.iter()
            .map(|(src, name)| format!("{src} AS {}", sql::ident(name)))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// A matrix: one query per level of the row and column hierarchies (each recomputes
/// its values, so subtotals of non-additive measures are right), stacked in one SQL
/// statement.
fn matrix_sql(
    model: &Model,
    schema: &Schema,
    v: &charted_model::Visual,
    q: VisualQuery,
    value_names: &[String],
) -> Result<String, CoreError> {
    let rows = v.roles.get("rows").map(Vec::len).unwrap_or(0);
    let cols = v.roles.get("columns").map(Vec::len).unwrap_or(0);
    // `q.groups` holds the column hierarchy before the rows (roles are ordered by name).
    let col_groups: Vec<Projection> = q.groups[..cols].to_vec();
    let row_groups: Vec<Projection> = q.groups[cols..cols + rows].to_vec();
    let mut parts = Vec::new();
    for r in 0..=rows {
        for c in 0..=cols {
            if r + c == 0 && q.values.is_empty() {
                continue;
            }
            let mut sq = q.clone();
            sq.groups = row_groups[..r]
                .iter()
                .chain(&col_groups[..c])
                .cloned()
                .collect();
            sq.keys = true;
            // A sort on a value orders every level; otherwise groups ascend.
            sq.order
                .retain(|o| matches!(o.target, OrderTarget::Value(_)));
            let compiled = compile(model, schema, &sq)?;
            let mut picks = vec![
                format!("{r} AS \"level:rows\""),
                format!("{c} AS \"level:columns\""),
                "row_number() OVER () AS \"order\"".to_string(),
            ];
            for i in 0..rows {
                if i < r {
                    picks.push(format!("g{i} AS \"rows:{i}\""));
                    picks.push(format!("k{i} AS \"key:rows:{i}\""));
                } else {
                    picks.push(format!("NULL AS \"rows:{i}\""));
                    picks.push(format!("NULL AS \"key:rows:{i}\""));
                }
            }
            for i in 0..cols {
                if i < c {
                    picks.push(format!("g{0} AS \"columns:{i}\"", r + i));
                    picks.push(format!("k{0} AS \"key:columns:{i}\"", r + i));
                } else {
                    picks.push(format!("NULL AS \"columns:{i}\""));
                    picks.push(format!("NULL AS \"key:columns:{i}\""));
                }
            }
            for (i, n) in value_names.iter().enumerate() {
                picks.push(format!("v{i} AS {}", sql::ident(n)));
            }
            parts.push(format!(
                "SELECT {} FROM ({}) AS m",
                picks.join(", "),
                compiled.sql
            ));
        }
    }
    if parts.is_empty() {
        return Ok("SELECT 1 WHERE FALSE".into());
    }
    Ok(parts.join(" UNION ALL "))
}

/// A slicer's values: the distinct values of its field (as `field:0`, with
/// `key:field:0`) under the other filters, or for a range slicer its smallest and
/// largest values.
fn slicer_sql(
    model: &Model,
    schema: &Schema,
    v: &charted_model::Visual,
    q: VisualQuery,
    search: Option<&str>,
) -> Result<String, CoreError> {
    let Some(field) = v.roles.get("field").and_then(|f| f.first()) else {
        return Ok("SELECT 1 WHERE FALSE".into());
    };
    let field = Projection {
        aggregation: None,
        ..field.clone()
    };
    let style = v
        .objects
        .get("slicer")
        .and_then(|o| o.get("style"))
        .and_then(Value::as_str)
        .unwrap_or("list");
    if matches!(style, "between" | "relative") {
        let agg = |a| Projection {
            aggregation: Some(a),
            ..field.clone()
        };
        let rq = VisualQuery {
            groups: vec![],
            values: vec![
                agg(charted_model::Aggregation::Min),
                agg(charted_model::Aggregation::Max),
            ],
            filters: q.filters,
            security: q.security,
            ..Default::default()
        };
        let c = compile(model, schema, &rq)?;
        return Ok(format!(
            "SELECT v0 AS \"min\", v1 AS \"max\" FROM ({}) AS r",
            c.sql
        ));
    }
    let lq = VisualQuery {
        groups: vec![field],
        values: vec![],
        filters: q.filters,
        order: vec![],
        limit: Some(5_000),
        keys: true,
        security: q.security,
    };
    let search = search.map(str::trim).filter(|s| !s.is_empty());
    let Some(search) = search else {
        let c = compile(model, schema, &lq)?;
        return Ok(format!(
            "SELECT g0 AS \"field:0\", k0 AS \"key:field:0\" FROM ({}) AS s",
            c.sql
        ));
    };
    // The search applies before the limit, so it finds values past the first 5,000.
    let c = compile(model, schema, &VisualQuery { limit: None, ..lq })?;
    Ok(format!(
        "SELECT g0 AS \"field:0\", k0 AS \"key:field:0\" FROM ({}) AS s \
         WHERE contains(lower(CAST(g0 AS VARCHAR)), lower('{}')) LIMIT 5000",
        c.sql,
        search.replace('\'', "''")
    ))
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
    // A hierarchy is named by the levels shown.
    let drill = charted_model::drill_role(&v.visual_type)
        .and_then(|r| Some((r, v.roles.get(r)?.len())))
        .filter(|(_, n)| *n > 1);
    for (role, ps) in &v.roles {
        for (i, p) in ps.iter().enumerate() {
            if let Some((r, n)) = drill {
                let (start, depth, _) = v.drill_levels(n);
                if r == role && !(start..=depth).contains(&i) {
                    continue;
                }
            }
            title_part(model, p, &mut groups, &mut values);
        }
    }
    match (values.is_empty(), groups.is_empty()) {
        (false, false) => format!("{} by {}", values.join(" and "), groups.join(" and ")),
        (false, true) => values.join(" and "),
        (true, false) => groups.join(" and "),
        (true, true) => String::new(),
    }
}

fn title_part(model: &Model, p: &Projection, groups: &mut Vec<String>, values: &mut Vec<String>) {
    let name = projection_name(model, p);
    if matches!(p.field, FieldRef::Column { .. }) && p.aggregation.is_none() {
        groups.push(name);
    } else {
        values.push(name);
    }
}
