//! Field parameters: a calculated table of `( "label", NAMEOF ( field ), order )` rows
//! (Power BI writes them so). A visual using the table's label or field column shows
//! the fields picked in it instead (by slicers and filters on the table), or all of them
//! in order when nothing is picked.

use charted_dax::{ExprKind, parse};
use charted_model::{ColumnKind, FieldRef, FilterKind, Id, Model, Page, Projection, Table, Visual};
use std::collections::BTreeMap;

/// One choice of a field parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub label: String,
    /// What NAMEOF names, as `'Table'[Name]` (the field column's value).
    pub name: String,
    pub field: FieldRef,
    pub order: f64,
}

/// A field parameter's choices, in order, or `None` when `table` isn't one.
pub fn items(model: &Model, table: &Table) -> Option<Vec<Item>> {
    let expression = &table.calculated.as_ref()?.expression;
    let parsed = parse(expression);
    if !parsed.diagnostics.is_empty() {
        return None;
    }
    let ExprKind::List(rows) = &parsed.expr.kind else {
        return None;
    };
    let mut out = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        let ExprKind::Tuple(cells) = &r.kind else {
            return None;
        };
        let (ExprKind::Text(label), ExprKind::Call { name, args }) =
            (&cells.first()?.kind, &cells.get(1)?.kind)
        else {
            return None;
        };
        if !name.eq_ignore_ascii_case("NAMEOF") || args.len() != 1 {
            return None;
        }
        let ExprKind::Ref { table: tn, name } = &args[0].kind else {
            return None;
        };
        let (home, field) = resolve(model, tn.as_deref(), name)?;
        let order = match cells.get(2).map(|c| &c.kind) {
            Some(ExprKind::Number(n)) => *n,
            _ => i as f64,
        };
        let shown = match &field {
            FieldRef::Column { column, .. } => home.column(column)?.name.clone(),
            FieldRef::Measure { measure, .. } => home
                .measures
                .iter()
                .find(|m| &m.id == measure)?
                .name
                .clone(),
        };
        out.push(Item {
            label: label.clone(),
            name: format!(
                "'{}'[{}]",
                home.name.replace('\'', "''"),
                shown.replace(']', "]]")
            ),
            field,
            order,
        });
    }
    out.sort_by(|a, b| a.order.total_cmp(&b.order));
    Some(out)
}

/// A NAMEOF reference: a measure (anywhere when no table is named) or a column.
fn resolve<'a>(model: &'a Model, table: Option<&str>, name: &str) -> Option<(&'a Table, FieldRef)> {
    let measure = |t: &'a Table| {
        t.measures
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case(name))
            .map(|m| {
                (
                    t,
                    FieldRef::Measure {
                        table: t.id.clone(),
                        measure: m.id.clone(),
                    },
                )
            })
    };
    match table {
        None => model.tables.iter().find_map(measure),
        Some(tn) => {
            let t = model.table_by_name(tn)?;
            measure(t).or_else(|| {
                t.column_by_name(name).map(|c| {
                    (
                        t,
                        FieldRef::Column {
                            table: t.id.clone(),
                            column: c.id.clone(),
                        },
                    )
                })
            })
        }
    }
}

/// Which of a field parameter's columns `column` is: its label (`Value1`) or its
/// field names (`Value2`).
fn role_of(table: &Table, column: &Id) -> Option<usize> {
    match &table.column(column)?.kind {
        ColumnKind::Data { source_column } if source_column == "Value1" => Some(1),
        ColumnKind::Data { source_column } if source_column == "Value2" => Some(2),
        _ => None,
    }
}

/// The items picked by the filters and slicers that reach `visual` on `page`.
fn picked(model: &Model, page: &Page, visual: &Visual, table: &Table, all: &[Item]) -> Vec<Item> {
    let filters = model
        .report
        .filters
        .iter()
        .chain(&page.settings.filters)
        .chain(&visual.filters)
        .filter(|f| f.is_active())
        .map(|f| (f.field.field.clone(), f.filter.clone()))
        .chain(
            crate::visualq::slicer_filters(page, &visual.id)
                .into_iter()
                .map(|f| (f.field.field, f.filter)),
        );
    let mut keep: Vec<Item> = all.to_vec();
    for (field, filter) in filters {
        let FieldRef::Column { table: t, column } = &field else {
            continue;
        };
        if *t != table.id {
            continue;
        }
        let Some(which) = role_of(table, column) else {
            continue;
        };
        let FilterKind::Basic { values, exclude } = filter else {
            continue;
        };
        keep.retain(|i| {
            let v = if which == 1 { &i.label } else { &i.name };
            values.iter().any(|x| x.as_deref() == Some(v.as_str())) != exclude
        });
    }
    keep
}

/// `visual`'s roles with each field parameter replaced by the fields picked in it, or
/// `None` when it uses no field parameter.
pub fn expand(
    model: &Model,
    page: &Page,
    visual: &Visual,
) -> Option<BTreeMap<String, Vec<Projection>>> {
    // A slicer on the parameter lists its choices.
    if visual.is_slicer() {
        return None;
    }
    let mut changed = false;
    let mut roles = BTreeMap::new();
    for (role, ps) in &visual.roles {
        let mut out = Vec::new();
        for p in ps {
            let param = match &p.field {
                FieldRef::Column { table, column } => model
                    .table(table)
                    .filter(|t| role_of(t, column).is_some())
                    .and_then(|t| Some((t, items(model, t)?))),
                FieldRef::Measure { .. } => None,
            };
            match param {
                Some((t, all)) => {
                    changed = true;
                    out.extend(picked(model, page, visual, t, &all).into_iter().map(|i| {
                        Projection {
                            field: i.field,
                            aggregation: None,
                            display_name: None,
                        }
                    }));
                }
                None => out.push(p.clone()),
            }
        }
        roles.insert(role.clone(), out);
    }
    changed.then_some(roles)
}

/// The expanded roles of every report visual that uses a field parameter, by visual id.
pub fn expanded(model: &Model) -> BTreeMap<Id, BTreeMap<String, Vec<Projection>>> {
    model
        .report
        .pages
        .iter()
        .flat_map(|p| p.visuals.iter().map(move |v| (p, v)))
        .filter_map(|(p, v)| Some((v.id.clone(), expand(model, p, v)?)))
        .collect()
}
