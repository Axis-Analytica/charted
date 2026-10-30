//! Changing the model: measures, columns, tables, relationships, hierarchies, roles,
//! calculation groups and parameters. DAX is checked before anything is saved.

use charted_core::visualq;
use charted_model::{
    CalculationGroup, CalculationItem, Cardinality, Change, Column, ColumnKind, ColumnProperty,
    CrossFilter, DataType, ExpressionLanguage, FieldRef, Hierarchy, Level, Measure, Model,
    Relationship, Role, RoleFilter, Summarize, Table,
};
use serde_json::{Value, json};

use super::{
    Cx, Done, Fail, Res, Step, ToolDef, action, diagnostics, fail, opt_bool, opt_str, schema,
    str_arg, str_list,
};
use crate::builders::{self, NumericParameter, new_id};
use crate::names::{self, quote_name, quote_table};

pub fn defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "measure",
            description: "Adds, updates (expression, format, description, folder, hidden), renames or removes a measure. DAX is checked first; a measure that doesn't check isn't saved. format is a format string like \"#,0\", \"$#,0.00\" or \"0.0%\".".into(),
            input_schema: schema(
                json!({
                    "action": { "type": "string", "enum": ["add", "update", "rename", "remove"] },
                    "table": { "type": "string", "description": "Its home table (add; others find it by name)" },
                    "name": { "type": "string" }, "new_name": { "type": "string" },
                    "expression": { "type": "string" }, "format": { "type": "string" },
                    "description": { "type": "string" }, "folder": { "type": "string" }, "hidden": { "type": "boolean" }
                }),
                &["action", "name"],
            ),
        },
        ToolDef {
            name: "column",
            description: "Adds a calculated column (DAX per row), or changes a column: expression (calculated only), format, summarize_by (Default, None, Sum, Average, Min, Max, Count, DistinctCount), sort_by (another column of its table), data_category (e.g. City, Country, ImageUrl, WebUrl), folder, description, hidden, or renames or removes it (calculated columns only; data columns change through Power Query).".into(),
            input_schema: schema(
                json!({
                    "action": { "type": "string", "enum": ["add_calculated", "update", "rename", "remove"] },
                    "table": { "type": "string" }, "name": { "type": "string" }, "new_name": { "type": "string" },
                    "expression": { "type": "string" }, "format": { "type": "string" },
                    "summarize_by": { "type": "string" }, "sort_by": { "type": "string" }, "data_category": { "type": "string" },
                    "folder": { "type": "string" }, "description": { "type": "string" }, "hidden": { "type": "boolean" }
                }),
                &["action", "table", "name"],
            ),
        },
        ToolDef {
            name: "table",
            description: "Adds a calculated table (a DAX table expression, e.g. CALENDAR or SUMMARIZECOLUMNS), changes one's expression, or renames, hides, describes, marks as the date table (date_column) or removes a table (calculated tables, calculation groups and parameters only).".into(),
            input_schema: schema(
                json!({
                    "action": { "type": "string", "enum": ["add_calculated", "update", "rename", "remove"] },
                    "name": { "type": "string" }, "new_name": { "type": "string" },
                    "expression": { "type": "string" }, "hidden": { "type": "boolean" }, "description": { "type": "string" },
                    "date_table": { "type": "boolean", "description": "Mark (or unmark) as the date table" }
                }),
                &["action", "name"],
            ),
        },
        ToolDef {
            name: "relationship",
            description: "Adds, changes or removes a relationship. from is the many side ('Sales'[ProductKey]), to the one side ('Product'[ProductKey]). cardinality: many_to_one (default), one_to_one, many_to_many; direction: single (default) or both; active defaults to true.".into(),
            input_schema: schema(
                json!({
                    "action": { "type": "string", "enum": ["add", "update", "remove"] },
                    "from": { "type": "string" }, "to": { "type": "string" },
                    "cardinality": { "type": "string", "enum": ["many_to_one", "one_to_one", "many_to_many"] },
                    "direction": { "type": "string", "enum": ["single", "both"] }, "active": { "type": "boolean" }
                }),
                &["action", "from", "to"],
            ),
        },
        ToolDef {
            name: "hierarchy",
            description: "Adds, replaces the levels of, or removes a hierarchy: levels are columns of the table, top first.".into(),
            input_schema: schema(
                json!({
                    "action": { "type": "string", "enum": ["add", "update", "remove"] },
                    "table": { "type": "string" }, "name": { "type": "string" },
                    "levels": { "type": "array", "items": { "type": "string" } }
                }),
                &["action", "table", "name"],
            ),
        },
        ToolDef {
            name: "role",
            description: "Adds, updates or removes a row-level security role. filters maps table names to DAX conditions per row (e.g. {\"Store\": \"[Region] = \\\"Europe\\\"\"}).".into(),
            input_schema: schema(
                json!({
                    "action": { "type": "string", "enum": ["add", "update", "remove"] },
                    "name": { "type": "string" }, "description": { "type": "string" },
                    "filters": { "type": "object", "additionalProperties": { "type": "string" } }
                }),
                &["action", "name"],
            ),
        },
        ToolDef {
            name: "calculation_group",
            description: "Adds, updates or removes a calculation group: items apply to whatever measure a visual shows, with SELECTEDMEASURE () standing for it (e.g. YTD = CALCULATE ( SELECTEDMEASURE (), DATESYTD ( 'Date'[Date] ) )). Put the group's Name column on an axis or in a slicer to use it.".into(),
            input_schema: schema(
                json!({
                    "action": { "type": "string", "enum": ["add", "update", "remove"] },
                    "name": { "type": "string" }, "precedence": { "type": "integer" },
                    "items": { "type": "array", "items": { "type": "object", "properties": { "name": {"type": "string"}, "expression": {"type": "string"}, "format": {"type": "string", "description": "A DAX expression giving a format string, e.g. \"\\\"0.0%\\\"\""} }, "required": ["name", "expression"] } }
                }),
                &["action", "name"],
            ),
        },
        ToolDef {
            name: "parameter",
            description: "Adds a parameter. kind field: a field parameter whose slicer swaps the fields a visual shows (fields: measures or columns; labels optional). kind numeric: a what-if parameter (min, max, step, default; decimal for non-whole numbers) with a '<name> Value' measure to use in other measures.".into(),
            input_schema: schema(
                json!({
                    "kind": { "type": "string", "enum": ["field", "numeric"] }, "name": { "type": "string" },
                    "fields": { "type": "array", "items": { "type": "string" } },
                    "labels": { "type": "array", "items": { "type": "string" } },
                    "min": { "type": "number" }, "max": { "type": "number" }, "step": { "type": "number" }, "default": { "type": "number" },
                    "decimal": { "type": "boolean" }
                }),
                &["kind", "name"],
            ),
        },
    ]
}

pub fn call(cx: &Cx, name: &str, args: &Value) -> Option<Res<Done>> {
    Some(match name {
        "measure" => measure(cx, args),
        "column" => column(cx, args),
        "table" => table(cx, args),
        "relationship" => relationship(cx, args),
        "hierarchy" => hierarchy(cx, args),
        "role" => role(cx, args),
        "calculation_group" => calculation_group(cx, args),
        "parameter" => parameter(cx, args),
        _ => return None,
    })
}

/// What uses a field: visuals showing it and DAX naming it.
fn uses(m: &Model, f: &FieldRef, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    for p in &m.report.pages {
        for v in &p.visuals {
            if v.roles
                .values()
                .flatten()
                .chain(v.filters.iter().map(|x| &x.field))
                .any(|x| &x.field == f)
            {
                out.push(format!(
                    "visual \"{}\" on \"{}\"",
                    visualq::visual_title(m, v),
                    p.settings.name
                ));
            }
        }
    }
    let needle = quote_name(name).to_lowercase();
    for t in &m.tables {
        for me in &t.measures {
            if !matches!(f, FieldRef::Measure { measure, .. } if *measure == me.id)
                && me.expression.to_lowercase().contains(&needle)
            {
                out.push(format!("measure {}", quote_name(&me.name)));
            }
        }
        for c in &t.columns {
            if let ColumnKind::Calculated { expression, .. } = &c.kind
                && !matches!(f, FieldRef::Column { column, .. } if *column == c.id)
                && expression.to_lowercase().contains(&needle)
            {
                out.push(format!(
                    "column {}{}",
                    quote_table(&t.name),
                    quote_name(&c.name)
                ));
            }
        }
    }
    out
}

fn confirm_or_fail(cx: &Cx, what: &str) -> Res<()> {
    if cx.turn.confirm(what) {
        Ok(())
    } else {
        fail(format!(
            "the user didn't allow this ({what}); leave it as it is"
        ))
    }
}

fn find_measure<'a>(m: &'a Model, args: &Value) -> Res<(&'a Table, &'a Measure)> {
    let name = str_arg(args, "name")?;
    let spec = names::FieldSpec {
        table: opt_str(args, "table").map(str::to_string),
        name: name.trim_start_matches('[').trim_end_matches(']').into(),
        aggregation: None,
    };
    match names::resolve(m, &spec)?.field {
        FieldRef::Measure { table, measure } => {
            let t = m.table(&table).expect("resolved");
            Ok((
                t,
                t.measures
                    .iter()
                    .find(|x| x.id == measure)
                    .expect("resolved"),
            ))
        }
        _ => fail(format!("{name} is a column, not a measure")),
    }
}

fn check_measure(
    m: &Model,
    s: &charted_core::session::Session,
    src: &str,
    id: Option<&str>,
) -> Res<()> {
    match diagnostics(src, &visualq::check_dax(m, s.schema(), src, id).diagnostics) {
        Some(d) => fail(format!("the DAX doesn't check: {d}")),
        None => Ok(()),
    }
}

fn name_taken(t: &Table, name: &str, except: Option<&str>) -> bool {
    t.columns
        .iter()
        .any(|c| Some(c.id.as_str()) != except && c.name.eq_ignore_ascii_case(name))
        || t.measures
            .iter()
            .any(|x| Some(x.id.as_str()) != except && x.name.eq_ignore_ascii_case(name))
}

fn measure(cx: &Cx, args: &Value) -> Res<Done> {
    let act = action(args, &["add", "update", "rename", "remove"])?;
    if act == "remove" {
        let (t, id, name, used) = cx.with_model(|m, _| {
            let (t, me) = find_measure(m, args)?;
            let f = FieldRef::Measure {
                table: t.id.clone(),
                measure: me.id.clone(),
            };
            Ok((
                t.id.clone(),
                me.id.clone(),
                me.name.clone(),
                uses(m, &f, &me.name),
            ))
        })?;
        if !used.is_empty() {
            confirm_or_fail(
                cx,
                &format!(
                    "Delete the measure {} (used by {})",
                    quote_name(&name),
                    used.join(", ")
                ),
            )?;
        }
        return cx.change(|_, _| {
            Ok((
                vec![Change::RemoveMeasure {
                    table: t.clone(),
                    measure: id.clone(),
                }],
                Done {
                    text: format!("Removed the measure {}.", quote_name(&name)),
                    step: Some(Step::new("Removed measure %1", &[&quote_name(&name)])),
                },
            ))
        });
    }
    cx.change(|m, s| match act {
        "add" => {
            let t = names::find_table(m, str_arg(args, "table")?)?;
            let name = str_arg(args, "name")?
                .trim()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_string();
            if name_taken(t, &name, None)
                || m.tables.iter().any(|x| {
                    x.measures
                        .iter()
                        .any(|me| me.name.eq_ignore_ascii_case(&name))
                })
            {
                return fail(format!(
                    "there's a measure or column {} already; update it or pick another name",
                    quote_name(&name)
                ));
            }
            let expr = str_arg(args, "expression")?.trim().to_string();
            check_measure(m, s, &expr, None)?;
            let me = Measure {
                id: new_id(),
                name: name.clone(),
                expression: expr.clone(),
                language: ExpressionLanguage::Dax,
                description: opt_str(args, "description").map(str::to_string),
                format_string: opt_str(args, "format").map(str::to_string),
                display_folder: opt_str(args, "folder").map(str::to_string),
                hidden: opt_bool(args, "hidden").unwrap_or(false),
                extras: Default::default(),
            };
            Ok((
                vec![Change::AddMeasure {
                    table: t.id.clone(),
                    measure: me,
                    index: None,
                }],
                Done {
                    text: format!(
                        "Added the measure {} to {}.",
                        quote_name(&name),
                        quote_table(&t.name)
                    ),
                    step: Some(Step::new("Added measure %1", &[&quote_name(&name)]).detail(expr)),
                },
            ))
        }
        _ => {
            let (t, old) = find_measure(m, args)?;
            let mut me = old.clone();
            if act == "rename" {
                let n = str_arg(args, "new_name")?
                    .trim()
                    .trim_start_matches('[')
                    .trim_end_matches(']')
                    .to_string();
                if name_taken(t, &n, Some(&me.id)) {
                    return fail(format!("{} is taken", quote_name(&n)));
                }
                me.name = n;
            } else {
                if let Some(e) = opt_str(args, "expression") {
                    check_measure(m, s, e.trim(), Some(&me.id))?;
                    me.expression = e.trim().into();
                }
                if let Some(v) = args.get("format") {
                    me.format_string = v.as_str().filter(|s| !s.is_empty()).map(str::to_string);
                }
                if let Some(v) = opt_str(args, "description") {
                    me.description = Some(v.into());
                }
                if let Some(v) = opt_str(args, "folder") {
                    me.display_folder = Some(v.into());
                }
                if let Some(h) = opt_bool(args, "hidden") {
                    me.hidden = h;
                }
            }
            let shown = quote_name(&me.name);
            let detail = me.expression.clone();
            Ok((
                vec![Change::UpdateMeasure {
                    table: t.id.clone(),
                    measure: me,
                }],
                Done {
                    text: format!("Updated the measure {shown}."),
                    step: Some(Step::new("Changed measure %1", &[&shown]).detail(detail)),
                },
            ))
        }
    })
}

fn summarize(s: &str) -> Res<Summarize> {
    serde_json::from_value(Value::String(
        match s.to_ascii_lowercase().replace(['_', ' '], "").as_str() {
            "default" => "Default",
            "none" | "dontsummarize" => "None",
            "sum" => "Sum",
            "average" | "avg" => "Average",
            "min" => "Min",
            "max" => "Max",
            "count" => "Count",
            "distinctcount" => "DistinctCount",
            _ => return fail(format!("summarize_by {s} isn't Default, None, Sum, Average, Min, Max, Count or DistinctCount")),
        }
        .into(),
    ))
    .map_err(Fail::from)
}

fn column(cx: &Cx, args: &Value) -> Res<Done> {
    let act = action(args, &["add_calculated", "update", "rename", "remove"])?;
    if act == "remove" {
        let (t, c, name, used) = cx.with_model(|m, _| {
            let t = names::find_table(m, str_arg(args, "table")?)?;
            let n = str_arg(args, "name")?;
            let c = t.column_by_name(n.trim_start_matches('[').trim_end_matches(']')).ok_or_else(|| Fail(format!("{} has no column {n}", quote_table(&t.name))))?;
            if !matches!(c.kind, ColumnKind::Calculated { .. }) {
                return fail("only calculated columns can be removed here; a data column comes from the table's query (draft a Power Query change)");
            }
            let f = FieldRef::Column { table: t.id.clone(), column: c.id.clone() };
            Ok((t.id.clone(), c.id.clone(), format!("{}{}", quote_table(&t.name), quote_name(&c.name)), uses(m, &f, &c.name)))
        })?;
        if !used.is_empty() {
            confirm_or_fail(
                cx,
                &format!("Delete the column {name} (used by {})", used.join(", ")),
            )?;
        }
        return cx.change(|_, _| {
            Ok((
                vec![Change::RemoveColumn {
                    table: t.clone(),
                    column: c.clone(),
                }],
                Done {
                    text: format!("Removed the column {name}."),
                    step: Some(Step::new("Removed column %1", &[&name])),
                },
            ))
        });
    }
    cx.change(|m, _| {
        let t = names::find_table(m, str_arg(args, "table")?)?;
        let n = str_arg(args, "name")?.trim().trim_start_matches('[').trim_end_matches(']').to_string();
        if act == "add_calculated" {
            if name_taken(t, &n, None) {
                return fail(format!("{} has a column or measure {} already", quote_table(&t.name), quote_name(&n)));
            }
            let expr = str_arg(args, "expression")?.trim().to_string();
            if let Some(d) = diagnostics(&expr, &charted_dax::check_in(m, &expr, Some(&t.id)).1) {
                return fail(format!("the DAX doesn't check: {d}"));
            }
            let c = Column {
                id: new_id(),
                name: n.clone(),
                // The core works out the type from the expression.
                data_type: DataType::String,
                kind: ColumnKind::Calculated { expression: expr.clone(), language: ExpressionLanguage::Dax },
                description: opt_str(args, "description").map(str::to_string),
                format_string: opt_str(args, "format").map(str::to_string),
                summarize_by: opt_str(args, "summarize_by").map(summarize).transpose()?.unwrap_or_default(),
                data_category: opt_str(args, "data_category").map(str::to_string),
                sort_by: None,
                display_folder: opt_str(args, "folder").map(str::to_string),
                hidden: opt_bool(args, "hidden").unwrap_or(false),
                is_key: false,
                extras: Default::default(),
            };
            let shown = format!("{}{}", quote_table(&t.name), quote_name(&n));
            return Ok((
                vec![Change::AddColumn { table: t.id.clone(), column: c, index: None }],
                Done { text: format!("Added the calculated column {shown}."), step: Some(Step::new("Added column %1", &[&shown]).detail(expr)) },
            ));
        }
        let c = t.column_by_name(&n).ok_or_else(|| Fail(format!("{} has no column {}", quote_table(&t.name), quote_name(&n))))?;
        let set = |property| Change::SetColumn { table: t.id.clone(), column: c.id.clone(), property };
        let mut changes = vec![];
        if act == "rename" {
            let to = str_arg(args, "new_name")?.trim().to_string();
            if name_taken(t, &to, Some(&c.id)) {
                return fail(format!("{} is taken", quote_name(&to)));
            }
            changes.push(Change::RenameColumn { table: t.id.clone(), column: c.id.clone(), name: to });
        } else {
            if let Some(e) = opt_str(args, "expression") {
                if !matches!(c.kind, ColumnKind::Calculated { .. }) {
                    return fail("only a calculated column has an expression");
                }
                if let Some(d) = diagnostics(e, &charted_dax::check_in(m, e, Some(&t.id)).1) {
                    return fail(format!("the DAX doesn't check: {d}"));
                }
                changes.push(set(ColumnProperty::Expression(e.trim().into())));
            }
            if let Some(v) = args.get("format") {
                changes.push(set(ColumnProperty::FormatString(v.as_str().filter(|s| !s.is_empty()).map(str::to_string))));
            }
            if let Some(v) = opt_str(args, "summarize_by") {
                changes.push(set(ColumnProperty::SummarizeBy(summarize(v)?)));
            }
            if let Some(v) = args.get("sort_by") {
                let by = match v.as_str().filter(|s| !s.is_empty()) {
                    Some(b) => {
                        let b = b.trim_start_matches('[').trim_end_matches(']');
                        Some(t.column_by_name(b).ok_or_else(|| Fail(format!("{} has no column {}", quote_table(&t.name), quote_name(b))))?.id.clone())
                    }
                    None => None,
                };
                changes.push(set(ColumnProperty::SortBy(by)));
            }
            if let Some(v) = args.get("data_category") {
                changes.push(set(ColumnProperty::DataCategory(v.as_str().filter(|s| !s.is_empty()).map(str::to_string))));
            }
            if let Some(v) = opt_str(args, "folder") {
                changes.push(set(ColumnProperty::DisplayFolder(Some(v.into()))));
            }
            if let Some(v) = opt_str(args, "description") {
                changes.push(set(ColumnProperty::Description(Some(v.into()))));
            }
            if let Some(h) = opt_bool(args, "hidden") {
                changes.push(set(ColumnProperty::Hidden(h)));
            }
            if changes.is_empty() {
                return fail("nothing to change: give expression, format, summarize_by, sort_by, data_category, folder, description or hidden");
            }
        }
        let shown = format!("{}{}", quote_table(&t.name), quote_name(&c.name));
        Ok((changes, Done { text: format!("Updated the column {shown}."), step: Some(Step::new("Changed column %1", &[&shown])) }))
    })
}

fn table(cx: &Cx, args: &Value) -> Res<Done> {
    let act = action(args, &["add_calculated", "update", "rename", "remove"])?;
    if act == "remove" {
        let (id, name) = cx.with_model(|m, _| {
            let t = names::find_table(m, str_arg(args, "name")?)?;
            if t.calculated.is_none() && t.calculation_group.is_none() {
                return fail("only calculated tables (and calculation groups and parameters) can be removed here; a table loaded from a source goes with its query");
            }
            Ok((t.id.clone(), t.name.clone()))
        })?;
        confirm_or_fail(cx, &format!("Delete the table {}", quote_table(&name)))?;
        return cx.change(|_, _| {
            Ok((
                vec![Change::RemoveTable { table: id.clone() }],
                Done {
                    text: format!("Removed the table {}.", quote_table(&name)),
                    step: Some(Step::new("Removed table %1", &[&quote_table(&name)])),
                },
            ))
        });
    }
    cx.change(|m, _| {
        let name = str_arg(args, "name")?.trim().trim_matches('\'').to_string();
        if act == "add_calculated" {
            if m.table_by_name(&name).is_some() {
                return fail(format!("there's a table {} already", quote_table(&name)));
            }
            let expr = str_arg(args, "expression")?.trim().to_string();
            if let Some(d) = diagnostics(&expr, &charted_dax::check_table(m, &expr).1) {
                return fail(format!("the DAX doesn't check: {d}"));
            }
            let mut t = builders::field_parameter_table(&name, &[]);
            t.columns.clear();
            t.calculated = Some(charted_model::CalculatedTable { expression: expr.clone(), language: ExpressionLanguage::Dax });
            t.hidden = opt_bool(args, "hidden").unwrap_or(false);
            t.description = opt_str(args, "description").map(str::to_string);
            return Ok((
                vec![Change::AddTable { table: t, index: None }],
                Done { text: format!("Added the calculated table {}.", quote_table(&name)), step: Some(Step::new("Added table %1", &[&quote_table(&name)]).detail(expr)) },
            ));
        }
        let t = names::find_table(m, &name)?;
        let mut changes = vec![];
        if act == "rename" {
            let to = str_arg(args, "new_name")?.trim().to_string();
            if m.tables.iter().any(|x| x.id != t.id && x.name.eq_ignore_ascii_case(&to)) {
                return fail(format!("there's a table {} already", quote_table(&to)));
            }
            changes.push(Change::RenameTable { table: t.id.clone(), name: to });
        } else {
            if let Some(e) = opt_str(args, "expression") {
                if t.calculated.is_none() || t.calculation_group.is_some() {
                    return fail("only a calculated table has an expression (a calculation group's items change with calculation_group)");
                }
                if let Some(d) = diagnostics(e, &charted_dax::check_table(m, e).1) {
                    return fail(format!("the DAX doesn't check: {d}"));
                }
                changes.push(Change::SetTableExpression { table: t.id.clone(), expression: e.trim().into() });
            }
            if let Some(h) = opt_bool(args, "hidden") {
                changes.push(Change::SetTableHidden { table: t.id.clone(), hidden: h });
            }
            if let Some(d) = opt_str(args, "description") {
                changes.push(Change::SetTableDescription { table: t.id.clone(), description: Some(d.into()) });
            }
            if let Some(d) = opt_bool(args, "date_table") {
                changes.push(Change::SetDateTable { table: t.id.clone(), is_date_table: d });
            }
            if changes.is_empty() {
                return fail("nothing to change: give expression, hidden, description or date_table");
            }
        }
        let shown = quote_table(&t.name);
        Ok((changes, Done { text: format!("Updated the table {shown}."), step: Some(Step::new("Changed table %1", &[&shown])) }))
    })
}

fn column_ref<'a>(m: &'a Model, src: &str) -> Res<(&'a Table, &'a Column)> {
    match names::field(m, src)?.field {
        FieldRef::Column { table, column } => {
            let t = m.table(&table).expect("resolved");
            Ok((t, t.column(&column).expect("resolved")))
        }
        _ => fail(format!("{src} is a measure; relationships join columns")),
    }
}

fn find_relationship<'a>(
    m: &'a Model,
    from: (&Table, &Column),
    to: (&Table, &Column),
) -> Option<&'a Relationship> {
    m.relationships
        .iter()
        .find(|r| {
            (r.from_column == from.1.id && r.to_column == to.1.id)
                || (r.from_column == to.1.id && r.to_column == from.1.id)
        })
        .filter(|_| from.0.id != to.0.id || true)
}

fn relationship(cx: &Cx, args: &Value) -> Res<Done> {
    let act = action(args, &["add", "update", "remove"])?;
    let label = |m: &Model, r: &Relationship| {
        format!(
            "{} → {}",
            names::format_field(
                m,
                &FieldRef::Column {
                    table: r.from_table.clone(),
                    column: r.from_column.clone()
                }
            ),
            names::format_field(
                m,
                &FieldRef::Column {
                    table: r.to_table.clone(),
                    column: r.to_column.clone()
                }
            )
        )
    };
    if act == "remove" {
        let (id, shown) = cx.with_model(|m, _| {
            let (from, to) = (
                column_ref(m, str_arg(args, "from")?)?,
                column_ref(m, str_arg(args, "to")?)?,
            );
            let r = find_relationship(m, from, to)
                .ok_or_else(|| Fail("there's no relationship between those columns".into()))?;
            Ok((r.id.clone(), label(m, r)))
        })?;
        confirm_or_fail(cx, &format!("Delete the relationship {shown}"))?;
        return cx.change(|_, _| {
            Ok((
                vec![Change::RemoveRelationship {
                    relationship: id.clone(),
                }],
                Done {
                    text: format!("Removed the relationship {shown}."),
                    step: Some(Step::new("Removed relationship %1", &[&shown])),
                },
            ))
        });
    }
    cx.change(|m, _| {
        let (from, to) = (
            column_ref(m, str_arg(args, "from")?)?,
            column_ref(m, str_arg(args, "to")?)?,
        );
        let existing = find_relationship(m, from, to);
        let mut r = match (act, existing) {
            ("add", Some(_)) => return fail("those columns are related already; use update"),
            ("add", None) => Relationship {
                id: new_id(),
                from_table: from.0.id.clone(),
                from_column: from.1.id.clone(),
                to_table: to.0.id.clone(),
                to_column: to.1.id.clone(),
                from_cardinality: Cardinality::Many,
                to_cardinality: Cardinality::One,
                cross_filter: CrossFilter::Single,
                // Another active path between the tables makes this one inactive.
                active: !m.relationships.iter().any(|x| {
                    x.active
                        && ((x.from_table == from.0.id && x.to_table == to.0.id)
                            || (x.from_table == to.0.id && x.to_table == from.0.id))
                }),
                extras: Default::default(),
            },
            (_, Some(r)) => r.clone(),
            (_, None) => return fail("there's no relationship between those columns; use add"),
        };
        match opt_str(args, "cardinality") {
            Some("one_to_one") => {
                (r.from_cardinality, r.to_cardinality) = (Cardinality::One, Cardinality::One)
            }
            Some("many_to_many") => {
                (r.from_cardinality, r.to_cardinality) = (Cardinality::Many, Cardinality::Many)
            }
            Some("many_to_one") => {
                (r.from_cardinality, r.to_cardinality) = (Cardinality::Many, Cardinality::One)
            }
            Some(c) => {
                return fail(format!(
                    "cardinality {c} isn't many_to_one, one_to_one or many_to_many"
                ));
            }
            None => {}
        }
        match opt_str(args, "direction") {
            Some("both") => r.cross_filter = CrossFilter::Both,
            Some("single") => r.cross_filter = CrossFilter::Single,
            Some(d) => return fail(format!("direction {d} isn't single or both")),
            None => {}
        }
        if let Some(a) = opt_bool(args, "active") {
            r.active = a;
        }
        let shown = label(m, &r);
        let note = if r.active {
            ""
        } else {
            " It is inactive (another path joins those tables); USERELATIONSHIP uses it."
        };
        let (change, op) = if act == "add" {
            (
                Change::AddRelationship {
                    relationship: r,
                    index: None,
                },
                "Added relationship %1",
            )
        } else {
            (
                Change::UpdateRelationship { relationship: r },
                "Changed relationship %1",
            )
        };
        Ok((
            vec![change],
            Done {
                text: format!("{}.{note}", op.replace("%1", &shown)),
                step: Some(Step::new(op, &[&shown])),
            },
        ))
    })
}

fn hierarchy(cx: &Cx, args: &Value) -> Res<Done> {
    let act = action(args, &["add", "update", "remove"])?;
    cx.change(|m, _| {
        let t = names::find_table(m, str_arg(args, "table")?)?;
        let name = str_arg(args, "name")?.trim().to_string();
        let existing = t
            .hierarchies
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(&name));
        let shown = format!("{}{}", quote_table(&t.name), quote_name(&name));
        if act == "remove" {
            let h = existing.ok_or_else(|| Fail(format!("there's no hierarchy {shown}")))?;
            return Ok((
                vec![Change::RemoveHierarchy {
                    table: t.id.clone(),
                    hierarchy: h.id.clone(),
                }],
                Done {
                    text: format!("Removed the hierarchy {shown}."),
                    step: Some(Step::new("Removed hierarchy %1", &[&shown])),
                },
            ));
        }
        let levels = str_list(args, "levels")
            .iter()
            .map(|l| {
                let bare = l.trim().trim_start_matches('[').trim_end_matches(']');
                let bare = bare.rsplit('[').next().unwrap_or(bare);
                t.column_by_name(bare)
                    .map(|c| Level {
                        name: c.name.clone(),
                        column: c.id.clone(),
                    })
                    .ok_or_else(|| {
                        Fail(format!(
                            "{} has no column {}",
                            quote_table(&t.name),
                            quote_name(bare)
                        ))
                    })
            })
            .collect::<Res<Vec<_>>>()?;
        if levels.is_empty() {
            return fail("levels lists the table's columns, top first");
        }
        let detail = levels
            .iter()
            .map(|l| quote_name(&l.name))
            .collect::<Vec<_>>()
            .join(" > ");
        let (change, op) = match (act, existing) {
            ("add", Some(_)) => {
                return fail(format!("there's a hierarchy {shown} already; use update"));
            }
            ("add", None) => (
                Change::AddHierarchy {
                    table: t.id.clone(),
                    hierarchy: Hierarchy {
                        id: new_id(),
                        name,
                        description: None,
                        levels,
                        hidden: false,
                        display_folder: None,
                        extras: Default::default(),
                    },
                    index: None,
                },
                "Added hierarchy %1",
            ),
            (_, Some(h)) => (
                Change::UpdateHierarchy {
                    table: t.id.clone(),
                    hierarchy: Hierarchy {
                        levels,
                        ..h.clone()
                    },
                },
                "Changed hierarchy %1",
            ),
            (_, None) => return fail(format!("there's no hierarchy {shown}; use add")),
        };
        Ok((
            vec![change],
            Done {
                text: format!("{}: {detail}.", op.replace("%1", &shown)),
                step: Some(Step::new(op, &[&shown]).detail(detail)),
            },
        ))
    })
}

fn role(cx: &Cx, args: &Value) -> Res<Done> {
    let act = action(args, &["add", "update", "remove"])?;
    if act == "remove" {
        let (id, name) = cx.with_model(|m, _| {
            let n = str_arg(args, "name")?;
            let r = m
                .roles
                .iter()
                .find(|r| r.name.eq_ignore_ascii_case(n))
                .ok_or_else(|| Fail(format!("there's no role \"{n}\"")))?;
            Ok((r.id.clone(), r.name.clone()))
        })?;
        confirm_or_fail(cx, &format!("Delete the role \"{name}\""))?;
        return cx.change(|_, _| {
            Ok((
                vec![Change::RemoveRole { role: id.clone() }],
                Done {
                    text: format!("Removed the role \"{name}\"."),
                    step: Some(Step::new("Removed role \"%1\"", &[&name])),
                },
            ))
        });
    }
    cx.change(|m, _| {
        let name = str_arg(args, "name")?.trim().to_string();
        let existing = m.roles.iter().find(|r| r.name.eq_ignore_ascii_case(&name));
        let mut filters = existing.map(|r| r.filters.clone()).unwrap_or_default();
        if let Some(map) = args.get("filters").and_then(Value::as_object) {
            filters.clear();
            for (tn, e) in map {
                let t = names::find_table(m, tn)?;
                let e = e
                    .as_str()
                    .ok_or_else(|| Fail("each filter is DAX text".into()))?;
                if let Some(d) = diagnostics(e, &charted_core::roles::check_filter(m, &t.id, e)) {
                    return fail(format!(
                        "the filter on {} doesn't check: {d}",
                        quote_table(&t.name)
                    ));
                }
                filters.push(RoleFilter {
                    table: t.id.clone(),
                    expression: e.trim().into(),
                });
            }
        }
        let detail = filters
            .iter()
            .map(|f| {
                format!(
                    "{}: {}",
                    m.table(&f.table).map_or("?", |t| t.name.as_str()),
                    f.expression
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let (change, op) = match (act, existing) {
            ("add", Some(_)) => {
                return fail(format!("there's a role \"{name}\" already; use update"));
            }
            ("add", None) => (
                Change::AddRole {
                    role: Role {
                        id: new_id(),
                        name: name.clone(),
                        description: opt_str(args, "description").map(str::to_string),
                        filters,
                        members: vec![],
                        extras: Default::default(),
                    },
                    index: None,
                },
                "Added role \"%1\"",
            ),
            (_, Some(r)) => (
                Change::UpdateRole {
                    role: Role {
                        filters,
                        description: opt_str(args, "description")
                            .map(str::to_string)
                            .or(r.description.clone()),
                        ..r.clone()
                    },
                },
                "Changed role \"%1\"",
            ),
            (_, None) => return fail(format!("there's no role \"{name}\"; use add")),
        };
        Ok((
            vec![change],
            Done {
                text: format!("{}.", op.replace("%1", &name)),
                step: Some(Step::new(op, &[&name]).detail(detail)),
            },
        ))
    })
}

fn calculation_group(cx: &Cx, args: &Value) -> Res<Done> {
    let act = action(args, &["add", "update", "remove"])?;
    if act == "remove" {
        let (id, name) = cx.with_model(|m, _| {
            let t = names::find_table(m, str_arg(args, "name")?)?;
            if t.calculation_group.is_none() {
                return fail(format!(
                    "{} isn't a calculation group",
                    quote_table(&t.name)
                ));
            }
            Ok((t.id.clone(), t.name.clone()))
        })?;
        confirm_or_fail(
            cx,
            &format!("Delete the calculation group {}", quote_table(&name)),
        )?;
        return cx.change(|_, _| {
            Ok((
                vec![Change::RemoveTable { table: id.clone() }],
                Done {
                    text: format!("Removed {}.", quote_table(&name)),
                    step: Some(Step::new("Removed table %1", &[&quote_table(&name)])),
                },
            ))
        });
    }
    cx.change(|m, _| {
        let name = str_arg(args, "name")?.trim().trim_matches('\'').to_string();
        let items = match args.get("items").and_then(Value::as_array) {
            Some(list) => list
                .iter()
                .map(|i| {
                    let n = i.get("name").and_then(Value::as_str).ok_or_else(|| Fail("each item needs name".into()))?;
                    let e = i.get("expression").and_then(Value::as_str).ok_or_else(|| Fail("each item needs expression".into()))?;
                    if let Some(d) = diagnostics(e, &charted_dax::check_item(m, e)) {
                        return fail(format!("item \"{n}\" doesn't check: {d}"));
                    }
                    Ok(CalculationItem {
                        name: n.trim().into(),
                        expression: e.trim().into(),
                        format_string: i.get("format").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string),
                        description: None,
                    })
                })
                .collect::<Res<Vec<_>>>()?,
            None => vec![],
        };
        let mut seen = std::collections::HashSet::new();
        if items.iter().any(|i| !seen.insert(i.name.to_lowercase())) {
            return fail("two items have the same name");
        }
        let precedence = args.get("precedence").and_then(Value::as_i64).map(|p| p as i32);
        let detail = items.iter().map(|i| format!("{} = {}", i.name, i.expression)).collect::<Vec<_>>().join("\n");
        let shown = quote_table(&name);
        if act == "add" {
            if m.table_by_name(&name).is_some() {
                return fail(format!("there's a table {shown} already"));
            }
            if items.is_empty() {
                return fail("a calculation group needs items");
            }
            let t = builders::calculation_group_table(&name, CalculationGroup { precedence: precedence.unwrap_or(0), items });
            return Ok((
                vec![Change::AddTable { table: t, index: None }],
                Done {
                    text: format!("Added the calculation group {shown}; put {shown}[Name] on an axis or in a slicer to apply its items."),
                    step: Some(Step::new("Added calculation group %1", &[&shown]).detail(detail)),
                },
            ));
        }
        let t = names::find_table(m, &name)?;
        let Some(old) = &t.calculation_group else { return fail(format!("{shown} isn't a calculation group")) };
        let group = CalculationGroup {
            precedence: precedence.unwrap_or(old.precedence),
            items: if items.is_empty() { old.items.clone() } else { items },
        };
        Ok((
            vec![Change::SetCalculationGroup { table: t.id.clone(), group }],
            Done { text: format!("Updated the calculation group {shown}."), step: Some(Step::new("Changed calculation group %1", &[&shown]).detail(detail)) },
        ))
    })
}

fn parameter(cx: &Cx, args: &Value) -> Res<Done> {
    cx.change(|m, _| {
        let name = str_arg(args, "name")?.trim().to_string();
        if m.table_by_name(&name).is_some() {
            return fail(format!("there's a table {} already", quote_table(&name)));
        }
        let (t, op, detail, note) = match str_arg(args, "kind")? {
            "field" => {
                let fields = str_list(args, "fields");
                if fields.is_empty() {
                    return fail("fields lists the measures or columns to swap between");
                }
                let labels = str_list(args, "labels");
                let mut rows = vec![];
                for (i, f) in fields.iter().enumerate() {
                    let p = names::field(m, f)?;
                    let (tn, fname) = match &p.field {
                        FieldRef::Column { table, column } => {
                            let t = m.table(table).expect("resolved");
                            (
                                t.name.clone(),
                                t.column(column).expect("resolved").name.clone(),
                            )
                        }
                        FieldRef::Measure { table, measure } => {
                            let t = m.table(table).expect("resolved");
                            (
                                t.name.clone(),
                                t.measures
                                    .iter()
                                    .find(|x| &x.id == measure)
                                    .expect("resolved")
                                    .name
                                    .clone(),
                            )
                        }
                    };
                    let label = labels.get(i).cloned().unwrap_or_else(|| fname.clone());
                    rows.push((label, tn, fname));
                }
                let detail = rows
                    .iter()
                    .map(|r| r.0.clone())
                    .collect::<Vec<_>>()
                    .join(", ");
                (
                    builders::field_parameter_table(&name, &rows),
                    "Added field parameter %1",
                    detail,
                    format!(
                        "Put {}{} in a visual's role and a slicer of it on the page.",
                        quote_table(&name),
                        quote_name(&name)
                    ),
                )
            }
            "numeric" => {
                let n = |k: &str| {
                    args.get(k)
                        .and_then(Value::as_f64)
                        .ok_or_else(|| Fail(format!("{k} is required (a number)")))
                };
                let p = NumericParameter {
                    name: name.clone(),
                    decimal: opt_bool(args, "decimal").unwrap_or(false),
                    min: n("min")?,
                    max: n("max")?,
                    step: n("step")?,
                    default: n("default")?,
                };
                if let Some(problem) = p.problem() {
                    return fail(problem);
                }
                (
                    p.table(),
                    "Added parameter %1",
                    format!("{} to {} by {}", p.min, p.max, p.step),
                    format!(
                        "Use [{name} Value] in measures and a slicer of {}{}.",
                        quote_table(&name),
                        quote_name(&name)
                    ),
                )
            }
            k => return fail(format!("kind {k} isn't field or numeric")),
        };
        let shown = quote_table(&name);
        Ok((
            vec![Change::AddTable {
                table: t,
                index: None,
            }],
            Done {
                text: format!("Added the parameter {shown}. {note}"),
                step: Some(Step::new(op, &[&shown]).detail(detail)),
            },
        ))
    })
}
