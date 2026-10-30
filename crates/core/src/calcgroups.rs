//! Calculation groups: a visual whose query picks calculation items (with the group's
//! column on an axis, or a slicer or filter leaving one item) shows its measures with
//! those items applied.
//!
//! Each measure the visual shows becomes a measure of its own, applying the item
//! picked in the current filter context:
//! `SWITCH ( SELECTEDVALUE ( 'Time'[Name] ), "YTD", CALCULATE ( 'Sales'[Total], … ), …,
//! 'Sales'[Total] )`, so rows where no single item is picked (totals, subtotals) show
//! the measure itself. With several groups the higher precedence applies outermost.
//! Items apply to the measures a visual shows, not to the measures those use.

use charted_compiler::visual::{QueryFilter, VisualQuery};
use charted_dax::calcitem::{self, Selected};
use charted_dax::lexer::{Tok, lex};
use charted_model::{CalculationItem, ColumnKind, FieldRef, FilterKind, Id, Measure, Model, Table};

/// Which of a calculation group's columns `column` is: its item names (`Value1`) or
/// their places (`Value2`).
fn role_of(table: &Table, column: &Id) -> Option<usize> {
    match &table.column(column)?.kind {
        ColumnKind::Data { source_column } if source_column == "Value1" => Some(1),
        ColumnKind::Data { source_column } if source_column == "Value2" => Some(2),
        _ => None,
    }
}

/// The items `filters` leave of `table`'s group, or `None` when nothing filters it.
fn picked<'a>(
    table: &Table,
    items: &'a [CalculationItem],
    filters: &[QueryFilter],
) -> Option<Vec<(usize, &'a CalculationItem)>> {
    let mut filtered = false;
    let mut keep: Vec<(usize, &CalculationItem)> = items.iter().enumerate().collect();
    for f in filters {
        let FieldRef::Column { table: t, column } = &f.field.field else {
            continue;
        };
        if *t != table.id {
            continue;
        }
        let Some(which) = role_of(table, column) else {
            continue;
        };
        let FilterKind::Basic { values, exclude } = &f.filter else {
            continue;
        };
        filtered = true;
        keep.retain(|(i, it)| {
            let v = if which == 1 {
                it.name.clone()
            } else {
                i.to_string()
            };
            values.iter().any(|x| x.as_deref() == Some(v.as_str())) != *exclude
        });
    }
    filtered.then_some(keep)
}

/// A group that applies to the query: its column (to read the item picked) and the
/// items it may pick.
struct Active<'a> {
    column: String,
    precedence: i32,
    items: Vec<(usize, &'a CalculationItem)>,
}

fn active<'a>(model: &'a Model, q: &VisualQuery) -> Vec<Active<'a>> {
    let mut out = Vec::new();
    for t in &model.tables {
        let Some(g) = &t.calculation_group else {
            continue;
        };
        let Some(name) = t.columns.iter().find(|c| role_of(t, &c.id) == Some(1)) else {
            continue;
        };
        let on_axis = q.groups.iter().any(|p| {
            matches!(&p.field, FieldRef::Column { table, column } if *table == t.id && role_of(t, column).is_some())
        });
        // On an axis, each row picks its item; otherwise a filter must pick one.
        let items = match picked(t, &g.items, &q.filters) {
            Some(items) if on_axis || items.len() == 1 => items,
            None if on_axis => g.items.iter().enumerate().collect(),
            _ => continue,
        };
        if items.is_empty() {
            continue;
        }
        out.push(Active {
            column: format!("{}[{}]", quote_table(&t.name), quote_name(&name.name)),
            precedence: g.precedence,
            items,
        });
    }
    out.sort_by_key(|a| a.precedence);
    out
}

fn quote_table(name: &str) -> String {
    format!("'{}'", name.replace('\'', "''"))
}

fn quote_name(name: &str) -> String {
    name.replace(']', "]]")
}

/// A format string expression's value when it is a constant (`"0.0%"`).
fn constant_text(src: &str) -> Option<String> {
    match lex(src).as_slice() {
        [t, e] if matches!(e.tok, Tok::Eof) => match &t.tok {
            Tok::Text(s) => Some(s.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// The query with its measures replaced by ones applying the calculation items picked,
/// and the model those measures are added to; `None` when no group applies.
pub fn apply(model: &Model, q: &mut VisualQuery) -> Option<Model> {
    let groups = active(model, q);
    if groups.is_empty() {
        return None;
    }
    let mut out = model.clone();
    let mut changed = false;
    for p in &mut q.values {
        let FieldRef::Measure { table, measure } = &p.field else {
            continue;
        };
        let Some((t, m)) = model
            .table(table)
            .and_then(|t| Some((t, t.measures.iter().find(|m| m.id == *measure)?)))
        else {
            continue;
        };
        let mut expression = format!("{}[{}]", quote_table(&t.name), quote_name(&m.name));
        let mut format_string = m.format_string.clone();
        for g in &groups {
            let selected = Selected {
                reference: &format!("( {expression} )"),
                name: &m.name,
                format_string: format_string.as_deref(),
            };
            let branches: Vec<String> = g
                .items
                .iter()
                .map(|(_, it)| {
                    format!(
                        "{}, {}",
                        calcitem::text(&it.name),
                        calcitem::apply(&it.expression, &selected)
                    )
                })
                .collect();
            // One item picked for the whole visual: its format string, when a constant.
            if let [(_, it)] = g.items.as_slice()
                && let Some(f) = &it.format_string
                && let Some(f) = constant_text(&calcitem::apply(f, &selected))
            {
                format_string = Some(f);
            }
            expression = format!(
                "SWITCH ( SELECTEDVALUE ( {} ), {}, {expression} )",
                g.column,
                branches.join(", ")
            );
        }
        let id = format!("__calcgroup_{}", m.id);
        let home = out.table_mut(table).expect("found above");
        if !home.measures.iter().any(|x| x.id == id) {
            home.measures.push(Measure {
                id: id.clone(),
                name: id.clone(),
                expression,
                format_string,
                hidden: true,
                ..m.clone()
            });
        }
        p.field = FieldRef::Measure {
            table: table.clone(),
            measure: id,
        };
        changed = true;
    }
    changed.then_some(out)
}
