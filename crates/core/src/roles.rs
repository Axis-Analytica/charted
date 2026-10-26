//! Row-level security roles: checking their filters, and turning the roles a report is
//! viewed as (*View as*) into the row filters every report query carries.

use charted_compiler::visual::RowFilter;
use charted_dax::{Bound, Diagnostic, Span};
use charted_model::{Id, Model};

/// Problems with a role's filter on `table`: it must bind as a condition on the table's
/// rows, without measures, aggregations or variables (as the compiler requires).
pub fn check_filter(model: &Model, table: &str, expression: &str) -> Vec<Diagnostic> {
    let whole = Span {
        start: 0,
        end: expression.len(),
    };
    if expression.trim().is_empty() {
        return vec![Diagnostic::error(whole, "the filter is empty")];
    }
    if model.table(table).is_none() {
        return vec![Diagnostic::error(whole, "the table doesn't exist")];
    }
    let (bound, diagnostics) = charted_dax::check_in(model, expression, Some(table));
    let Some(bound) = bound else {
        return diagnostics;
    };
    if !diagnostics.is_empty() {
        return diagnostics;
    }
    let mut pure = true;
    charted_dax::bind::walk(&bound, &mut |x| {
        if matches!(
            x,
            Bound::Measure { .. }
                | Bound::Aggregate { .. }
                | Bound::Rank { .. }
                | Bound::ConcatX { .. }
                | Bound::Calculate { .. }
                | Bound::Info { .. }
                | Bound::Var(_)
                | Bound::Let { .. }
        ) {
            pure = false;
        }
    });
    if pure {
        Vec::new()
    } else {
        vec![Diagnostic::error(
            whole,
            "a role filter is a condition on each row: it can't use measures, aggregations or variables",
        )]
    }
}

/// The row filters for viewing the report as `roles` (ids; unknown ones are ignored).
///
/// Several roles combine as a union, as in Power BI: a row is visible if any of the
/// roles lets it through. Per table, the roles' conditions are joined with `||`; a
/// table that one of the roles doesn't filter is unrestricted. This is exact for
/// filters on the same table; for roles filtering different tables it is wider than
/// Power BI's union, which also lets through rows of a fact table that any one role
/// sees through its own combination of tables.
pub fn security(model: &Model, roles: &[Id]) -> Vec<RowFilter> {
    let active: Vec<&charted_model::Role> = model
        .roles
        .iter()
        .filter(|r| roles.contains(&r.id))
        .collect();
    let Some(first) = active.first() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for f in &first.filters {
        if model.table(&f.table).is_none() {
            continue;
        }
        let mut parts = Vec::new();
        for r in &active {
            match r.filters.iter().find(|g| g.table == f.table) {
                Some(g) => parts.push(g.expression.trim().to_string()),
                None => {
                    parts.clear();
                    break;
                }
            }
        }
        if parts.is_empty() {
            continue;
        }
        parts.dedup();
        let expression = if parts.len() == 1 {
            parts.remove(0)
        } else {
            parts
                .iter()
                .map(|p| format!("( {p} )"))
                .collect::<Vec<_>>()
                .join(" || ")
        };
        out.push(RowFilter {
            table: f.table.clone(),
            expression,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use charted_model::{Role, RoleFilter};

    fn role(id: &str, filters: &[(&str, &str)]) -> Role {
        Role {
            id: id.into(),
            name: id.into(),
            description: None,
            filters: filters
                .iter()
                .map(|(t, e)| RoleFilter {
                    table: t.to_string(),
                    expression: e.to_string(),
                })
                .collect(),
            members: vec![],
            extras: Default::default(),
        }
    }

    fn model() -> Model {
        let table = |id: &str| charted_model::Table {
            id: id.into(),
            name: id.into(),
            description: None,
            source: id.into(),
            columns: vec![],
            measures: vec![],
            hierarchies: vec![],
            hidden: false,
            is_date_table: false,
            extras: Default::default(),
        };
        Model {
            tables: vec![table("Store"), table("Product")],
            roles: vec![
                role("north", &[("Store", "A"), ("Product", "P")]),
                role("south", &[("Store", "B")]),
                role("all", &[]),
                role("gone", &[("Missing", "X")]),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn roles_union_per_table() {
        let m = model();
        let f = |ids: &[&str]| -> Vec<(String, String)> {
            let ids: Vec<Id> = ids.iter().map(|s| s.to_string()).collect();
            security(&m, &ids)
                .into_iter()
                .map(|r| (r.table, r.expression))
                .collect()
        };
        assert_eq!(f(&[]), vec![]);
        assert_eq!(
            f(&["north"]),
            vec![("Store".into(), "A".into()), ("Product".into(), "P".into())]
        );
        // Product is unrestricted for "south", so for the union too.
        assert_eq!(
            f(&["north", "south"]),
            vec![("Store".into(), "( A ) || ( B )".into())]
        );
        // A role without filters sees everything.
        assert_eq!(f(&["north", "all"]), vec![]);
        assert_eq!(f(&["gone"]), vec![]);
        assert_eq!(f(&["unknown"]), vec![]);
    }
}
