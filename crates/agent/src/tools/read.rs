//! Reading the project: the model and report described, visual types, DAX checks, and
//! numbers (visuals' aggregates always; rows and values only with data access).

use charted_compiler::visual::VisualQuery;
use charted_core::visualq;
use charted_model::{ColumnKind, FieldRef, Model, Projection};
use serde_json::{Value, json};
use std::fmt::Write;

use super::{Cx, Done, Fail, Res, ToolDef, fail, opt_str, schema, str_arg, str_list};
use crate::catalog;
use crate::names::{self, format_projection, quote_name, quote_table};

pub fn defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "describe_model",
            description: "The semantic model: tables (rows), columns (type, format, hidden, calculated expression), measures (DAX, format), hierarchies, relationships, roles, calculation groups and field parameters. Call it first. Pass table to see one table only.".into(),
            input_schema: schema(
                json!({ "table": { "type": "string", "description": "Only this table" } }),
                &[],
            ),
        },
        ToolDef {
            name: "describe_report",
            description: "The report: its pages (name, size, visual count), theme, bookmarks and report filters. With page (name or id): that page's visuals with ids, types, titles, positions, fields, filters and formatting.".into(),
            input_schema: schema(json!({ "page": { "type": "string" } }), &[]),
        },
        ToolDef {
            name: "list_visual_types",
            description: "Visual types you can add, with their roles (where fields go: grouping roles take columns, value roles take measures or aggregated columns), sizes and, for one type, its formatting properties.".into(),
            input_schema: schema(
                json!({ "type": { "type": "string", "description": "One type, with its format properties" } }),
                &[],
            ),
        },
        ToolDef {
            name: "check_dax",
            description: "Checks a DAX expression without saving it. kind: measure (default), column (needs table), table, role_filter (needs table), calc_item.".into(),
            input_schema: schema(
                json!({
                    "expression": { "type": "string" },
                    "kind": { "type": "string", "enum": ["measure", "column", "table", "role_filter", "calc_item"] },
                    "table": { "type": "string" }
                }),
                &["expression"],
            ),
        },
        ToolDef {
            name: "query_visual",
            description: "The numbers a visual shows (by id or title), as rows named after its fields, to check your work. Without data access only visuals that aggregate (a measure or SUM/COUNT…) answer, up to 50 rows.".into(),
            input_schema: schema(
                json!({ "visual": { "type": "string" }, "limit": { "type": "integer", "minimum": 1, "maximum": 100 } }),
                &["visual"],
            ),
        },
        ToolDef {
            name: "query_fields",
            description: "Aggregates without making a visual: values (measures or SUM('T'[C]) …) by group_by columns. Without data access at least one value is needed and up to 50 rows come back.".into(),
            input_schema: schema(
                json!({
                    "group_by": { "type": "array", "items": { "type": "string" } },
                    "values": { "type": "array", "items": { "type": "string" } },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                }),
                &[],
            ),
        },
        ToolDef {
            name: "sample_rows",
            description: "Distinct combinations of a table's columns (all, or those named), up to 50. Needs data access.".into(),
            input_schema: schema(
                json!({ "table": { "type": "string" }, "columns": { "type": "array", "items": { "type": "string" } }, "limit": { "type": "integer", "minimum": 1, "maximum": 50 } }),
                &["table"],
            ),
        },
        ToolDef {
            name: "distinct_values",
            description: "A column's values (for filters and slicers), optionally those containing search, up to 100. Needs data access.".into(),
            input_schema: schema(
                json!({ "field": { "type": "string" }, "search": { "type": "string" }, "limit": { "type": "integer", "minimum": 1, "maximum": 100 } }),
                &["field"],
            ),
        },
    ]
}

pub fn call(cx: &Cx, name: &str, args: &Value) -> Option<Res<Done>> {
    Some(match name {
        "describe_model" => {
            cx.with_model(|m, s| Done::read(describe_model(m, s, opt_str(args, "table"))?))
        }
        "describe_report" => {
            cx.with_model(|m, _| Done::read(describe_report(m, opt_str(args, "page"))?))
        }
        "list_visual_types" => list_visual_types(opt_str(args, "type")),
        "check_dax" => cx.with_model(|m, s| check_dax(m, s, args)),
        "query_visual" => query_visual(cx, args),
        "query_fields" => query_fields(cx, args),
        "sample_rows" => sample_rows(cx, args),
        "distinct_values" => distinct_values(cx, args),
        _ => return None,
    })
}

fn kind_text(c: &charted_model::Column) -> String {
    let mut s = format!("{:?}", c.data_type).to_lowercase();
    if let Some(f) = &c.format_string {
        let _ = write!(s, " fmt \"{f}\"");
    }
    if c.hidden {
        s.push_str(" hidden");
    }
    if c.is_key {
        s.push_str(" key");
    }
    s
}

pub fn describe_model(
    m: &Model,
    s: &charted_core::session::Session,
    only: Option<&str>,
) -> Res<String> {
    let mut out = String::new();
    let tables: Vec<_> = match only {
        Some(t) => vec![names::find_table(m, t)?],
        None => m.tables.iter().collect(),
    };
    for t in tables {
        let rows = if s.database().table_exists(&t.source).unwrap_or(false) {
            s.database()
                .query_u64(&format!(
                    "SELECT count(*) FROM {}",
                    charted_engine::sql::ident(&t.source)
                ))
                .unwrap_or(0)
        } else {
            0
        };
        let mut head = format!("{} ({rows} rows)", quote_table(&t.name));
        if t.hidden {
            head.push_str(" hidden");
        }
        if t.is_date_table {
            head.push_str(" date table");
        }
        if let Some(g) = &t.calculation_group {
            let items = g
                .items
                .iter()
                .map(|i| format!("\"{}\" = {}", i.name, i.expression))
                .collect::<Vec<_>>()
                .join("; ");
            let _ = write!(
                head,
                " calculation group (precedence {}): {items}",
                g.precedence
            );
        } else if let Some(c) = &t.calculated {
            if charted_core::fieldparams::items(m, t).is_some() {
                head.push_str(" field parameter");
            }
            let _ = write!(head, " calculated = {}", c.expression.replace('\n', " "));
        }
        let _ = writeln!(out, "{head}");
        if let Some(d) = &t.description {
            let _ = writeln!(out, "  description: {d}");
        }
        let cols: Vec<String> = t
            .columns
            .iter()
            .map(|c| {
                let mut s = format!("{} {}", quote_name(&c.name), kind_text(c));
                if let ColumnKind::Calculated { expression, .. } = &c.kind {
                    let _ = write!(s, " = {}", expression.replace('\n', " "));
                }
                if let Some(by) = c.sort_by.as_ref().and_then(|id| t.column(id)) {
                    let _ = write!(s, " sorted by {}", quote_name(&by.name));
                }
                s
            })
            .collect();
        if !cols.is_empty() {
            let _ = writeln!(out, "  columns: {}", cols.join(" | "));
        }
        for me in &t.measures {
            let mut s = format!(
                "  measure {} = {}",
                quote_name(&me.name),
                me.expression.replace('\n', " ")
            );
            if let Some(f) = &me.format_string {
                let _ = write!(s, "  fmt \"{f}\"");
            }
            if me.hidden {
                s.push_str(" hidden");
            }
            if let Some(f) = &me.display_folder {
                let _ = write!(s, " folder {f}");
            }
            let _ = writeln!(out, "{s}");
        }
        for h in &t.hierarchies {
            let levels = h
                .levels
                .iter()
                .map(|l| quote_name(t.column(&l.column).map_or(&l.name, |c| &c.name)))
                .collect::<Vec<_>>()
                .join(" > ");
            let _ = writeln!(out, "  hierarchy {} = {levels}", quote_name(&h.name));
        }
    }
    if only.is_none() {
        for r in &m.relationships {
            let field = |t: &str, c: &str| {
                names::format_field(
                    m,
                    &FieldRef::Column {
                        table: t.into(),
                        column: c.into(),
                    },
                )
            };
            let card = |c: charted_model::Cardinality| {
                if c == charted_model::Cardinality::One {
                    "1"
                } else {
                    "*"
                }
            };
            let _ = writeln!(
                out,
                "relationship {} {}→{} {}{}{}",
                field(&r.from_table, &r.from_column),
                card(r.from_cardinality),
                card(r.to_cardinality),
                field(&r.to_table, &r.to_column),
                if r.cross_filter == charted_model::CrossFilter::Both {
                    " both directions"
                } else {
                    ""
                },
                if r.active { "" } else { " inactive" }
            );
        }
        for r in &m.roles {
            let filters = r
                .filters
                .iter()
                .map(|f| {
                    format!(
                        "{}: {}",
                        m.table(&f.table).map_or(f.table.as_str(), |t| &t.name),
                        f.expression
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            let _ = writeln!(out, "role \"{}\": {filters}", r.name);
        }
    }
    Ok(out)
}

fn projections(m: &Model, ps: &[Projection]) -> String {
    ps.iter()
        .map(|p| format_projection(m, p))
        .collect::<Vec<_>>()
        .join(", ")
}

fn filter_text(m: &Model, f: &charted_model::ReportFilter) -> String {
    format!(
        "{} {}",
        format_projection(m, &f.field),
        serde_json::to_string(&f.filter).unwrap_or_default()
    )
}

pub fn describe_report(m: &Model, page: Option<&str>) -> Res<String> {
    let r = &m.report;
    let mut out = String::new();
    let Some(key) = page else {
        for (i, p) in r.pages.iter().enumerate() {
            let s = &p.settings;
            let _ = writeln!(
                out,
                "{i}. \"{}\" id {} {}×{}, {} visuals{}{}",
                s.name,
                p.id,
                s.width,
                s.height,
                p.visuals.len(),
                if s.hidden { ", hidden" } else { "" },
                if s.tooltip { ", tooltip page" } else { "" }
            );
        }
        let _ = writeln!(
            out,
            "theme: {}",
            r.theme
                .as_ref()
                .map_or("follows the desktop (Omarchy)", |t| t.name.as_str())
        );
        for f in &r.filters {
            let _ = writeln!(out, "report filter: {}", filter_text(m, f));
        }
        for b in &r.bookmarks {
            let _ = writeln!(out, "bookmark \"{}\" (page {})", b.name, b.page);
        }
        return Ok(out);
    };
    let p = names::find_page(m, key)?;
    let s = &p.settings;
    let _ = writeln!(
        out,
        "page \"{}\" id {} {}×{}",
        s.name, p.id, s.width, s.height
    );
    for f in &s.filters {
        let _ = writeln!(out, "page filter: {}", filter_text(m, f));
    }
    for v in &p.visuals {
        let pos = &v.position;
        let _ = writeln!(
            out,
            "- {} \"{}\" id {} at x{} y{} w{} h{}{}",
            v.visual_type,
            visualq::visual_title(m, v),
            v.id,
            pos.x,
            pos.y,
            pos.width,
            pos.height,
            if v.hidden { " hidden" } else { "" }
        );
        for (role, ps) in &v.roles {
            if !ps.is_empty() {
                let _ = writeln!(out, "    {role}: {}", projections(m, ps));
            }
        }
        for f in &v.filters {
            let _ = writeln!(out, "    filter: {}", filter_text(m, f));
        }
        if let Some(sort) = &v.sort {
            let _ = writeln!(
                out,
                "    sort: {} {} {}",
                sort.role,
                sort.index,
                if sort.descending {
                    "descending"
                } else {
                    "ascending"
                }
            );
        }
        if !v.objects.is_empty() {
            let _ = writeln!(out, "    format: {}", Value::Object(v.objects.clone()));
        }
    }
    Ok(out)
}

fn list_visual_types(ty: Option<&str>) -> Res<Done> {
    if let Some(t) = ty {
        let Some(v) = catalog::visual(t) else {
            return fail(format!(
                "there's no visual type {t}; types: {}",
                catalog::types()
            ));
        };
        return Done::read(v.describe(true));
    }
    let mut out = catalog::catalog()
        .visuals
        .iter()
        .map(|v| v.describe(false))
        .collect::<Vec<_>>()
        .join("\n");
    let themes = catalog::catalog()
        .themes
        .iter()
        .filter_map(|t| t.get("name").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join(", ");
    let _ = write!(out, "\nbuilt-in themes: {themes}");
    Done::read(out)
}

fn check_dax(m: &Model, s: &charted_core::session::Session, args: &Value) -> Res<Done> {
    let src = str_arg(args, "expression")?;
    let kind = opt_str(args, "kind").unwrap_or("measure");
    let table = || -> Res<&charted_model::Table> {
        names::find_table(m, str_arg(args, "table")?).map_err(Fail::from)
    };
    let ds = match kind {
        "measure" => visualq::check_dax(m, s.schema(), src, None).diagnostics,
        "column" => charted_dax::check_in(m, src, Some(&table()?.id)).1,
        "table" => charted_dax::check_table(m, src).1,
        "role_filter" => charted_core::roles::check_filter(m, &table()?.id, src),
        "calc_item" => charted_dax::check_item(m, src),
        k => {
            return fail(format!(
                "kind {k} isn't one of measure, column, table, role_filter, calc_item"
            ));
        }
    };
    Done::read(super::diagnostics(src, &ds).unwrap_or_else(|| "OK: no problems.".into()))
}

fn limit(args: &Value, data: bool) -> u64 {
    let max = if data { 100 } else { 50 };
    args.get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(max)
        .clamp(1, max)
}

/// Rows as compact JSON lines, or a note when there are none.
fn rows_text(rows: Vec<String>, limit: u64) -> String {
    if rows.is_empty() {
        return "(no rows)".into();
    }
    let n = rows.len();
    let mut out = rows.join("\n");
    if n as u64 >= limit {
        let _ = write!(out, "\n(first {limit} rows)");
    }
    out
}

fn aggregates(p: &Projection) -> bool {
    p.aggregation.is_some() || matches!(p.field, FieldRef::Measure { .. })
}

fn query_visual(cx: &Cx, args: &Value) -> Res<Done> {
    let data = cx.turn.data();
    let limit = limit(args, data);
    let (db, sql) = cx.with_model(|m, s| {
        let (_, v) = names::find_visual(m, None, str_arg(args, "visual")?)?;
        if !data && !v.roles.values().flatten().any(aggregates) {
            return fail(format!(
                "{} \"{}\" shows rows or values rather than totals, and data access is off; query a visual that aggregates (a measure or SUM/COUNT …), or ask the user to turn on \"Let the agent see data\".",
                v.visual_type,
                visualq::visual_title(m, v)
            ));
        }
        let sql = visualq::report_visual_sql(
            m,
            s.schema(),
            &v.id,
            &visualq::VisualSqlOptions {
                captions: true,
                ..Default::default()
            },
        )?;
        Ok((s.database().clone(), sql))
    })?;
    Done::read(rows_text(db.json_rows(&sql, Some(limit))?, limit))
}

fn resolve_all(m: &Model, list: &[String]) -> Res<Vec<Projection>> {
    list.iter()
        .map(|f| names::field(m, f).map_err(Fail::from))
        .collect()
}

fn query_fields(cx: &Cx, args: &Value) -> Res<Done> {
    let data = cx.turn.data();
    let limit = limit(args, data);
    let (db, sql, header) = cx.with_model(|m, s| {
        let groups = resolve_all(m, &str_list(args, "group_by"))?;
        let mut values = resolve_all(m, &str_list(args, "values"))?;
        if let Some(g) = groups.iter().find(|p| p.aggregation.is_some() || matches!(p.field, FieldRef::Measure { .. })) {
            return fail(format!("group_by takes columns; {} goes in values", format_projection(m, g)));
        }
        for v in &mut values {
            if let (FieldRef::Column { table, column }, None) = (&v.field, v.aggregation) {
                let c = m.table(table).and_then(|t| t.column(column));
                v.aggregation = Some(if c.is_some_and(|c| c.data_type.is_numeric()) {
                    charted_model::Aggregation::Sum
                } else {
                    charted_model::Aggregation::Count
                });
            }
        }
        if values.is_empty() && !data {
            return fail("give at least one value (a measure or SUM/COUNT …): data access is off, so values alone can't be listed");
        }
        if groups.is_empty() && values.is_empty() {
            return fail("give group_by columns, values, or both");
        }
        let header: Vec<String> = groups.iter().chain(&values).map(|p| format_projection(m, p)).collect();
        let q = VisualQuery {
            groups,
            values,
            limit: Some(limit as u32),
            ..Default::default()
        };
        let c = visualq::compile(m, s.schema(), &q)?;
        Ok((s.database().clone(), c.sql, header))
    })?;
    let rows = db.json_rows(&sql, Some(limit))?;
    Done::read(format!(
        "columns g0…, v0… are: {}\n{}",
        header.join(", "),
        rows_text(rows, limit)
    ))
}

fn sample_rows(cx: &Cx, args: &Value) -> Res<Done> {
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(20)
        .clamp(1, 50);
    let (db, sql, header) = cx.with_model(|m, s| {
        let t = names::find_table(m, str_arg(args, "table")?)?;
        let wanted = str_list(args, "columns");
        let cols: Vec<_> = if wanted.is_empty() {
            t.columns.iter().filter(|c| !c.hidden).take(12).collect()
        } else {
            wanted
                .iter()
                .map(|n| {
                    let bare = n.trim().trim_start_matches('[').trim_end_matches(']');
                    t.column_by_name(bare).ok_or_else(|| {
                        Fail(format!(
                            "{} has no column {}",
                            quote_table(&t.name),
                            quote_name(bare)
                        ))
                    })
                })
                .collect::<Res<_>>()?
        };
        let groups: Vec<Projection> = cols
            .iter()
            .map(|c| Projection {
                field: FieldRef::Column {
                    table: t.id.clone(),
                    column: c.id.clone(),
                },
                aggregation: None,
                display_name: None,
            })
            .collect();
        let header = cols
            .iter()
            .map(|c| quote_name(&c.name))
            .collect::<Vec<_>>()
            .join(", ");
        let q = VisualQuery {
            groups,
            limit: Some(limit as u32),
            ..Default::default()
        };
        Ok((
            s.database().clone(),
            visualq::compile(m, s.schema(), &q)?.sql,
            header,
        ))
    })?;
    Done::read(format!(
        "g0… are {header}\n{}",
        rows_text(db.json_rows(&sql, Some(limit))?, limit)
    ))
}

fn distinct_values(cx: &Cx, args: &Value) -> Res<Done> {
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(50)
        .clamp(1, 100) as u32;
    let (db, sql) = cx.with_model(|m, s| {
        let p = names::field(m, str_arg(args, "field")?)?;
        let sql =
            visualq::field_values_sql(m, s.schema(), &p, &[], &[], opt_str(args, "search"), limit)?;
        Ok((s.database().clone(), sql))
    })?;
    let v = visualq::run_field_values(&db, &sql)?;
    let list = v
        .values
        .iter()
        .map(|x| x.value.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    Done::read(format!("{} values in all; {list}", v.total))
}
