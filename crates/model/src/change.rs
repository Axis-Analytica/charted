use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::report::*;
use crate::types::*;

/// One edit to the model. Every view expresses edits this way, which is what makes
/// undo, autosave and multi-view sync uniform.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum Change {
    AddTable {
        table: Table,
        index: Option<usize>,
    },
    RemoveTable {
        table: Id,
    },
    RenameTable {
        table: Id,
        name: String,
    },
    SetTableDescription {
        table: Id,
        description: Option<String>,
    },
    SetTableHidden {
        table: Id,
        hidden: bool,
    },
    SetDateTable {
        table: Id,
        is_date_table: bool,
    },
    /// A calculated table's DAX expression (the session re-syncs its columns).
    SetTableExpression {
        table: Id,
        expression: String,
    },
    /// A calculation group's items and precedence (its table's rows follow them).
    SetCalculationGroup {
        table: Id,
        group: CalculationGroup,
    },
    RenameColumn {
        table: Id,
        column: Id,
        name: String,
    },
    SetColumn {
        table: Id,
        column: Id,
        property: ColumnProperty,
    },
    AddColumn {
        table: Id,
        column: Column,
        index: Option<usize>,
    },
    RemoveColumn {
        table: Id,
        column: Id,
    },
    AddMeasure {
        table: Id,
        measure: Measure,
        index: Option<usize>,
    },
    RemoveMeasure {
        table: Id,
        measure: Id,
    },
    /// Replaces a measure wholesale (name, expression, format…), keeping its id.
    UpdateMeasure {
        table: Id,
        measure: Measure,
    },
    AddHierarchy {
        table: Id,
        hierarchy: Hierarchy,
        index: Option<usize>,
    },
    RemoveHierarchy {
        table: Id,
        hierarchy: Id,
    },
    /// Replaces a hierarchy wholesale (name, levels, folder…), keeping its id.
    UpdateHierarchy {
        table: Id,
        hierarchy: Hierarchy,
    },
    AddRelationship {
        relationship: Relationship,
        index: Option<usize>,
    },
    RemoveRelationship {
        relationship: Id,
    },
    UpdateRelationship {
        relationship: Relationship,
    },
    AddRole {
        role: Role,
        index: Option<usize>,
    },
    RemoveRole {
        role: Id,
    },
    /// Replaces a role wholesale (name, description, filters, members), keeping its id.
    UpdateRole {
        role: Role,
    },
    AddPage {
        page: Page,
        index: Option<usize>,
    },
    RemovePage {
        page: Id,
    },
    /// Replaces a page's name, size and filters (not its visuals).
    SetPage {
        page: Id,
        settings: PageSettings,
    },
    MovePage {
        page: Id,
        index: usize,
    },
    AddVisual {
        page: Id,
        visual: Visual,
        index: Option<usize>,
    },
    RemoveVisual {
        page: Id,
        visual: Id,
    },
    /// Replaces a visual wholesale (position, fields, filters, formatting), keeping its id.
    UpdateVisual {
        page: Id,
        visual: Visual,
    },
    SetReportFilters {
        filters: Vec<ReportFilter>,
    },
    /// Replaces the model view's card positions.
    SetDiagram {
        positions: std::collections::BTreeMap<Id, DiagramPosition>,
    },
    /// Sets the report theme (`None` follows the desktop theme).
    SetTheme {
        theme: Option<ReportTheme>,
    },
    /// Replaces the report's bookmarks.
    SetBookmarks {
        bookmarks: Vec<Bookmark>,
    },
}

/// A single column property, so one change type covers the whole properties pane.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "name", content = "value", rename_all = "camelCase")]
#[ts(export)]
pub enum ColumnProperty {
    Description(Option<String>),
    FormatString(Option<String>),
    SummarizeBy(Summarize),
    DataCategory(Option<String>),
    SortBy(Option<Id>),
    DisplayFolder(Option<String>),
    Hidden(bool),
    IsKey(bool),
    DataType(DataType),
    /// A calculated column's DAX expression.
    Expression(String),
    /// What charted keeps on the column without interpreting it, such as the settings
    /// of the group dialog that wrote a group column's expression.
    Extras(#[ts(type = "Record<string, unknown>")] Extras),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize, TS)]
#[serde(
    tag = "code",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum ChangeError {
    #[error("there is no table with id {id}")]
    UnknownTable { id: Id },
    #[error("there is no column with id {id}")]
    UnknownColumn { id: Id },
    #[error("there is no measure with id {id}")]
    UnknownMeasure { id: Id },
    #[error("there is no hierarchy with id {id}")]
    UnknownHierarchy { id: Id },
    #[error("there is no relationship with id {id}")]
    UnknownRelationship { id: Id },
    #[error("there is no role with id {id}")]
    UnknownRole { id: Id },
    #[error("there is no page with id {id}")]
    UnknownPage { id: Id },
    #[error("there is no visual with id {id}")]
    UnknownVisual { id: Id },
    #[error("an object with id {id} already exists")]
    DuplicateId { id: Id },
    #[error("the name \"{name}\" is already used")]
    NameTaken { name: String },
    #[error("\"{name}\" is not a valid name: {reason}")]
    InvalidName { name: String, reason: String },
    #[error("{message}")]
    Invalid { message: String },
}

type Result<T> = std::result::Result<T, ChangeError>;

fn validate_name(name: &str) -> Result<()> {
    let bad = |reason: &str| {
        Err(ChangeError::InvalidName {
            name: name.to_string(),
            reason: reason.to_string(),
        })
    };
    if name.trim().is_empty() {
        return bad("names cannot be empty");
    }
    if name.trim() != name {
        return bad("names cannot start or end with spaces");
    }
    if name.chars().any(char::is_control) {
        return bad("names cannot contain control characters");
    }
    if name.chars().count() > 100 {
        return bad("names are limited to 100 characters");
    }
    Ok(())
}

/// At least one item, each named (uniquely) and with an expression.
fn validate_calculation_group(g: &CalculationGroup) -> Result<()> {
    if g.items.is_empty() {
        return Err(ChangeError::Invalid {
            message: "a calculation group needs at least one item".into(),
        });
    }
    for (i, it) in g.items.iter().enumerate() {
        validate_name(&it.name)?;
        if g.items[..i]
            .iter()
            .any(|o| o.name.eq_ignore_ascii_case(&it.name))
        {
            return Err(ChangeError::NameTaken {
                name: it.name.clone(),
            });
        }
        if it.expression.trim().is_empty() {
            return Err(ChangeError::Invalid {
                message: format!("the calculation item \"{}\" needs an expression", it.name),
            });
        }
    }
    Ok(())
}

fn table_mut<'a>(model: &'a mut Model, id: &str) -> Result<&'a mut Table> {
    model
        .table_mut(id)
        .ok_or_else(|| ChangeError::UnknownTable { id: id.to_string() })
}

fn index_of<T>(
    items: &[T],
    pred: impl Fn(&T) -> bool,
    unknown: impl FnOnce() -> ChangeError,
) -> Result<usize> {
    items.iter().position(pred).ok_or_else(unknown)
}

fn insert_at<T>(items: &mut Vec<T>, index: Option<usize>, item: T) {
    let i = index.unwrap_or(items.len()).min(items.len());
    items.insert(i, item);
}

fn ids_in_use(model: &Model) -> impl Iterator<Item = &str> {
    model
        .tables
        .iter()
        .flat_map(|t| {
            std::iter::once(t.id.as_str())
                .chain(t.columns.iter().map(|c| c.id.as_str()))
                .chain(t.measures.iter().map(|m| m.id.as_str()))
                .chain(t.hierarchies.iter().map(|h| h.id.as_str()))
        })
        .chain(model.relationships.iter().map(|r| r.id.as_str()))
        .chain(model.roles.iter().map(|r| r.id.as_str()))
        .chain(model.report.pages.iter().flat_map(|p| {
            std::iter::once(p.id.as_str()).chain(p.visuals.iter().map(|v| v.id.as_str()))
        }))
}

/// A role's name is unique among roles, and it filters each table at most once.
fn validate_role(model: &Model, r: &Role) -> Result<()> {
    validate_name(&r.name)?;
    if model
        .roles
        .iter()
        .any(|o| o.id != r.id && o.name.eq_ignore_ascii_case(&r.name))
    {
        return Err(ChangeError::NameTaken {
            name: r.name.clone(),
        });
    }
    for (i, f) in r.filters.iter().enumerate() {
        if model.table(&f.table).is_none() {
            return Err(ChangeError::UnknownTable {
                id: f.table.clone(),
            });
        }
        if r.filters[..i].iter().any(|o| o.table == f.table) {
            return Err(ChangeError::Invalid {
                message: "a role filters each table once".into(),
            });
        }
    }
    Ok(())
}

fn page_mut<'a>(model: &'a mut Model, id: &str) -> Result<&'a mut Page> {
    model
        .report
        .page_mut(id)
        .ok_or_else(|| ChangeError::UnknownPage { id: id.to_string() })
}

fn validate_page_name(model: &Model, id: &str, name: &str) -> Result<()> {
    validate_name(name)?;
    if model
        .report
        .pages
        .iter()
        .any(|p| p.id != id && p.settings.name.eq_ignore_ascii_case(name))
    {
        return Err(ChangeError::NameTaken {
            name: name.to_string(),
        });
    }
    Ok(())
}

fn ensure_fresh_id(model: &Model, id: &str) -> Result<()> {
    if ids_in_use(model).any(|x| x == id) {
        return Err(ChangeError::DuplicateId { id: id.to_string() });
    }
    Ok(())
}

fn validate_hierarchy(t: &Table, h: &Hierarchy) -> Result<()> {
    validate_name(&h.name)?;
    if t.has_field_named(&h.name, Some(&h.id)) {
        return Err(ChangeError::NameTaken {
            name: h.name.clone(),
        });
    }
    if h.levels.is_empty() {
        return Err(ChangeError::Invalid {
            message: "a hierarchy needs at least one level".into(),
        });
    }
    for (i, l) in h.levels.iter().enumerate() {
        validate_name(&l.name)?;
        if t.column(&l.column).is_none() {
            return Err(ChangeError::UnknownColumn {
                id: l.column.clone(),
            });
        }
        let earlier = &h.levels[..i];
        if earlier.iter().any(|o| o.name.eq_ignore_ascii_case(&l.name)) {
            return Err(ChangeError::NameTaken {
                name: l.name.clone(),
            });
        }
        if earlier.iter().any(|o| o.column == l.column) {
            return Err(ChangeError::Invalid {
                message: "a column can be only one level of a hierarchy".into(),
            });
        }
    }
    Ok(())
}

fn validate_relationship(model: &Model, r: &Relationship) -> Result<()> {
    let from = model
        .table(&r.from_table)
        .ok_or_else(|| ChangeError::UnknownTable {
            id: r.from_table.clone(),
        })?;
    let to = model
        .table(&r.to_table)
        .ok_or_else(|| ChangeError::UnknownTable {
            id: r.to_table.clone(),
        })?;
    if from.column(&r.from_column).is_none() {
        return Err(ChangeError::UnknownColumn {
            id: r.from_column.clone(),
        });
    }
    if to.column(&r.to_column).is_none() {
        return Err(ChangeError::UnknownColumn {
            id: r.to_column.clone(),
        });
    }
    if r.from_table == r.to_table {
        return Err(ChangeError::Invalid {
            message: "a relationship must connect two different tables".into(),
        });
    }
    let duplicate = model.relationships.iter().any(|o| {
        o.id != r.id
            && ((o.from_column == r.from_column && o.to_column == r.to_column)
                || (o.from_column == r.to_column && o.to_column == r.from_column))
    });
    if duplicate {
        return Err(ChangeError::Invalid {
            message: "these columns are already related".into(),
        });
    }
    Ok(())
}

/// Applies `change` to `model` and returns the changes that exactly undo it, in the
/// order they must be applied. On error the model is left untouched.
pub fn apply(model: &mut Model, change: Change) -> Result<Vec<Change>> {
    match change {
        Change::AddTable { mut table, index } => {
            validate_name(&table.name)?;
            if let Some(g) = &table.calculation_group {
                validate_calculation_group(g)?;
                // Its rows are its items.
                table.calculated = Some(CalculatedTable {
                    expression: calculation_group_expression(g),
                    language: ExpressionLanguage::Dax,
                });
            }
            if model.table_by_name(&table.name).is_some() {
                return Err(ChangeError::NameTaken { name: table.name });
            }
            ensure_fresh_id(model, &table.id)?;
            let id = table.id.clone();
            insert_at(&mut model.tables, index, table);
            Ok(vec![Change::RemoveTable { table: id }])
        }
        Change::RemoveTable { table } => {
            let i = index_of(
                &model.tables,
                |t| t.id == table,
                || ChangeError::UnknownTable { id: table.clone() },
            )?;
            // Relationships touching the table go with it; undo restores them afterwards.
            let mut inverse = Vec::new();
            let removed_rels: Vec<(usize, Relationship)> = model
                .relationships
                .iter()
                .enumerate()
                .filter(|(_, r)| r.from_table == table || r.to_table == table)
                .map(|(i, r)| (i, r.clone()))
                .collect();
            model
                .relationships
                .retain(|r| r.from_table != table && r.to_table != table);
            let t = model.tables.remove(i);
            inverse.push(Change::AddTable {
                table: t,
                index: Some(i),
            });
            inverse.extend(removed_rels.into_iter().map(|(i, relationship)| {
                Change::AddRelationship {
                    relationship,
                    index: Some(i),
                }
            }));
            // So do the roles' filters on it.
            for r in model.roles.iter_mut() {
                if r.filters.iter().any(|f| f.table == table) {
                    inverse.push(Change::UpdateRole { role: r.clone() });
                    r.filters.retain(|f| f.table != table);
                }
            }
            Ok(inverse)
        }
        Change::RenameTable { table, name } => {
            validate_name(&name)?;
            if model
                .tables
                .iter()
                .any(|t| t.id != table && t.name.eq_ignore_ascii_case(&name))
            {
                return Err(ChangeError::NameTaken { name });
            }
            let t = table_mut(model, &table)?;
            let old = std::mem::replace(&mut t.name, name);
            Ok(vec![Change::RenameTable { table, name: old }])
        }
        Change::SetTableDescription { table, description } => {
            let t = table_mut(model, &table)?;
            let old = std::mem::replace(&mut t.description, description);
            Ok(vec![Change::SetTableDescription {
                table,
                description: old,
            }])
        }
        Change::SetTableHidden { table, hidden } => {
            let t = table_mut(model, &table)?;
            let old = std::mem::replace(&mut t.hidden, hidden);
            Ok(vec![Change::SetTableHidden { table, hidden: old }])
        }
        Change::SetDateTable {
            table,
            is_date_table,
        } => {
            let t = table_mut(model, &table)?;
            let old = std::mem::replace(&mut t.is_date_table, is_date_table);
            Ok(vec![Change::SetDateTable {
                table,
                is_date_table: old,
            }])
        }
        Change::SetTableExpression { table, expression } => {
            let t = table_mut(model, &table)?;
            let Some(calc) = t.calculated.as_mut() else {
                return Err(ChangeError::Invalid {
                    message: format!("\"{}\" is not a calculated table", t.name),
                });
            };
            let old = std::mem::replace(&mut calc.expression, expression);
            Ok(vec![Change::SetTableExpression {
                table,
                expression: old,
            }])
        }
        Change::SetCalculationGroup { table, group } => {
            validate_calculation_group(&group)?;
            let t = table_mut(model, &table)?;
            let (Some(old), Some(calc)) = (t.calculation_group.as_mut(), t.calculated.as_mut())
            else {
                return Err(ChangeError::Invalid {
                    message: format!("\"{}\" is not a calculation group", t.name),
                });
            };
            calc.expression = calculation_group_expression(&group);
            let old = std::mem::replace(old, group);
            Ok(vec![Change::SetCalculationGroup { table, group: old }])
        }
        Change::RenameColumn {
            table,
            column,
            name,
        } => {
            validate_name(&name)?;
            let t = table_mut(model, &table)?;
            if t.has_field_named(&name, Some(&column)) {
                return Err(ChangeError::NameTaken { name });
            }
            let c = t
                .column_mut(&column)
                .ok_or_else(|| ChangeError::UnknownColumn { id: column.clone() })?;
            let old = std::mem::replace(&mut c.name, name);
            Ok(vec![Change::RenameColumn {
                table,
                column,
                name: old,
            }])
        }
        Change::SetColumn {
            table,
            column,
            property,
        } => {
            let t = table_mut(model, &table)?;
            if let ColumnProperty::SortBy(Some(target)) = &property {
                if target == &column {
                    return Err(ChangeError::Invalid {
                        message: "a column cannot be sorted by itself".into(),
                    });
                }
                if t.column(target).is_none() {
                    return Err(ChangeError::UnknownColumn { id: target.clone() });
                }
            }
            let c = t
                .column_mut(&column)
                .ok_or_else(|| ChangeError::UnknownColumn { id: column.clone() })?;
            let old = match property {
                ColumnProperty::Description(v) => {
                    ColumnProperty::Description(std::mem::replace(&mut c.description, v))
                }
                ColumnProperty::FormatString(v) => {
                    ColumnProperty::FormatString(std::mem::replace(&mut c.format_string, v))
                }
                ColumnProperty::SummarizeBy(v) => {
                    ColumnProperty::SummarizeBy(std::mem::replace(&mut c.summarize_by, v))
                }
                ColumnProperty::DataCategory(v) => {
                    ColumnProperty::DataCategory(std::mem::replace(&mut c.data_category, v))
                }
                ColumnProperty::SortBy(v) => {
                    ColumnProperty::SortBy(std::mem::replace(&mut c.sort_by, v))
                }
                ColumnProperty::DisplayFolder(v) => {
                    ColumnProperty::DisplayFolder(std::mem::replace(&mut c.display_folder, v))
                }
                ColumnProperty::Hidden(v) => {
                    ColumnProperty::Hidden(std::mem::replace(&mut c.hidden, v))
                }
                ColumnProperty::IsKey(v) => {
                    ColumnProperty::IsKey(std::mem::replace(&mut c.is_key, v))
                }
                ColumnProperty::DataType(v) => {
                    ColumnProperty::DataType(std::mem::replace(&mut c.data_type, v))
                }
                ColumnProperty::Extras(v) => {
                    ColumnProperty::Extras(std::mem::replace(&mut c.extras, v))
                }
                ColumnProperty::Expression(v) => match &mut c.kind {
                    ColumnKind::Calculated { expression, .. } => {
                        ColumnProperty::Expression(std::mem::replace(expression, v))
                    }
                    ColumnKind::Data { .. } => {
                        return Err(ChangeError::Invalid {
                            message: "only calculated columns have an expression".into(),
                        });
                    }
                },
            };
            Ok(vec![Change::SetColumn {
                table,
                column,
                property: old,
            }])
        }
        Change::AddColumn {
            table,
            column,
            index,
        } => {
            validate_name(&column.name)?;
            ensure_fresh_id(model, &column.id)?;
            let t = table_mut(model, &table)?;
            if t.has_field_named(&column.name, None) {
                return Err(ChangeError::NameTaken { name: column.name });
            }
            let id = column.id.clone();
            insert_at(&mut t.columns, index, column);
            Ok(vec![Change::RemoveColumn { table, column: id }])
        }
        Change::RemoveColumn { table, column } => {
            let in_relationship = model
                .relationships
                .iter()
                .any(|r| r.from_column == column || r.to_column == column);
            if in_relationship {
                return Err(ChangeError::Invalid {
                    message: "remove the relationships that use this column first".into(),
                });
            }
            let t = table_mut(model, &table)?;
            if t.columns
                .iter()
                .any(|c| c.sort_by.as_deref() == Some(column.as_str()))
            {
                return Err(ChangeError::Invalid {
                    message: "another column is sorted by this column".into(),
                });
            }
            let i = index_of(
                &t.columns,
                |c| c.id == column,
                || ChangeError::UnknownColumn { id: column.clone() },
            )?;
            let c = t.columns.remove(i);
            let mut inverse = vec![Change::AddColumn {
                table: table.clone(),
                column: c,
                index: Some(i),
            }];
            // The column leaves the hierarchies it is a level of; an emptied hierarchy goes.
            let mut k = 0;
            while k < t.hierarchies.len() {
                let h = &mut t.hierarchies[k];
                if !h.levels.iter().any(|l| l.column == column) {
                    k += 1;
                    continue;
                }
                let old = h.clone();
                h.levels.retain(|l| l.column != column);
                if h.levels.is_empty() {
                    t.hierarchies.remove(k);
                    inverse.push(Change::AddHierarchy {
                        table: table.clone(),
                        hierarchy: old,
                        index: Some(k),
                    });
                } else {
                    inverse.push(Change::UpdateHierarchy {
                        table: table.clone(),
                        hierarchy: old,
                    });
                    k += 1;
                }
            }
            Ok(inverse)
        }
        Change::AddMeasure {
            table,
            measure,
            index,
        } => {
            validate_name(&measure.name)?;
            ensure_fresh_id(model, &measure.id)?;
            let t = table_mut(model, &table)?;
            if t.has_field_named(&measure.name, None) {
                return Err(ChangeError::NameTaken { name: measure.name });
            }
            let id = measure.id.clone();
            insert_at(&mut t.measures, index, measure);
            Ok(vec![Change::RemoveMeasure { table, measure: id }])
        }
        Change::RemoveMeasure { table, measure } => {
            let t = table_mut(model, &table)?;
            let i = index_of(
                &t.measures,
                |m| m.id == measure,
                || ChangeError::UnknownMeasure {
                    id: measure.clone(),
                },
            )?;
            let m = t.measures.remove(i);
            Ok(vec![Change::AddMeasure {
                table,
                measure: m,
                index: Some(i),
            }])
        }
        Change::UpdateMeasure { table, measure } => {
            validate_name(&measure.name)?;
            let t = table_mut(model, &table)?;
            if t.has_field_named(&measure.name, Some(&measure.id)) {
                return Err(ChangeError::NameTaken { name: measure.name });
            }
            let id = measure.id.clone();
            let m = t
                .measure_mut(&id)
                .ok_or(ChangeError::UnknownMeasure { id })?;
            let old = std::mem::replace(m, measure);
            Ok(vec![Change::UpdateMeasure {
                table,
                measure: old,
            }])
        }
        Change::AddHierarchy {
            table,
            hierarchy,
            index,
        } => {
            ensure_fresh_id(model, &hierarchy.id)?;
            let t = table_mut(model, &table)?;
            validate_hierarchy(t, &hierarchy)?;
            let id = hierarchy.id.clone();
            insert_at(&mut t.hierarchies, index, hierarchy);
            Ok(vec![Change::RemoveHierarchy {
                table,
                hierarchy: id,
            }])
        }
        Change::RemoveHierarchy { table, hierarchy } => {
            let t = table_mut(model, &table)?;
            let i = index_of(
                &t.hierarchies,
                |h| h.id == hierarchy,
                || ChangeError::UnknownHierarchy {
                    id: hierarchy.clone(),
                },
            )?;
            let h = t.hierarchies.remove(i);
            Ok(vec![Change::AddHierarchy {
                table,
                hierarchy: h,
                index: Some(i),
            }])
        }
        Change::UpdateHierarchy { table, hierarchy } => {
            let t = table_mut(model, &table)?;
            validate_hierarchy(t, &hierarchy)?;
            let id = hierarchy.id.clone();
            let h = t
                .hierarchies
                .iter_mut()
                .find(|h| h.id == id)
                .ok_or(ChangeError::UnknownHierarchy { id })?;
            let old = std::mem::replace(h, hierarchy);
            Ok(vec![Change::UpdateHierarchy {
                table,
                hierarchy: old,
            }])
        }
        Change::AddRelationship {
            relationship,
            index,
        } => {
            ensure_fresh_id(model, &relationship.id)?;
            validate_relationship(model, &relationship)?;
            let id = relationship.id.clone();
            insert_at(&mut model.relationships, index, relationship);
            Ok(vec![Change::RemoveRelationship { relationship: id }])
        }
        Change::RemoveRelationship { relationship } => {
            let i = index_of(
                &model.relationships,
                |r| r.id == relationship,
                || ChangeError::UnknownRelationship {
                    id: relationship.clone(),
                },
            )?;
            let r = model.relationships.remove(i);
            Ok(vec![Change::AddRelationship {
                relationship: r,
                index: Some(i),
            }])
        }
        Change::UpdateRelationship { relationship } => {
            validate_relationship(model, &relationship)?;
            let id = relationship.id.clone();
            let r = model
                .relationships
                .iter_mut()
                .find(|r| r.id == id)
                .ok_or(ChangeError::UnknownRelationship { id })?;
            let old = std::mem::replace(r, relationship);
            Ok(vec![Change::UpdateRelationship { relationship: old }])
        }
        Change::AddRole { role, index } => {
            ensure_fresh_id(model, &role.id)?;
            validate_role(model, &role)?;
            let id = role.id.clone();
            insert_at(&mut model.roles, index, role);
            Ok(vec![Change::RemoveRole { role: id }])
        }
        Change::RemoveRole { role } => {
            let i = index_of(
                &model.roles,
                |r| r.id == role,
                || ChangeError::UnknownRole { id: role.clone() },
            )?;
            let r = model.roles.remove(i);
            Ok(vec![Change::AddRole {
                role: r,
                index: Some(i),
            }])
        }
        Change::UpdateRole { role } => {
            validate_role(model, &role)?;
            let id = role.id.clone();
            let r = model
                .roles
                .iter_mut()
                .find(|r| r.id == id)
                .ok_or(ChangeError::UnknownRole { id })?;
            let old = std::mem::replace(r, role);
            Ok(vec![Change::UpdateRole { role: old }])
        }
        Change::AddPage { page, index } => {
            validate_page_name(model, &page.id, &page.settings.name)?;
            ensure_fresh_id(model, &page.id)?;
            for v in &page.visuals {
                ensure_fresh_id(model, &v.id)?;
            }
            let id = page.id.clone();
            insert_at(&mut model.report.pages, index, page);
            Ok(vec![Change::RemovePage { page: id }])
        }
        Change::RemovePage { page } => {
            let i = index_of(
                &model.report.pages,
                |p| p.id == page,
                || ChangeError::UnknownPage { id: page.clone() },
            )?;
            let p = model.report.pages.remove(i);
            Ok(vec![Change::AddPage {
                page: p,
                index: Some(i),
            }])
        }
        Change::SetPage { page, settings } => {
            validate_page_name(model, &page, &settings.name)?;
            let p = page_mut(model, &page)?;
            let old = std::mem::replace(&mut p.settings, settings);
            Ok(vec![Change::SetPage {
                page,
                settings: old,
            }])
        }
        Change::MovePage { page, index } => {
            let pages = &mut model.report.pages;
            let from = index_of(
                pages,
                |p| p.id == page,
                || ChangeError::UnknownPage { id: page.clone() },
            )?;
            let p = pages.remove(from);
            let to = index.min(pages.len());
            pages.insert(to, p);
            Ok(vec![Change::MovePage { page, index: from }])
        }
        Change::AddVisual {
            page,
            visual,
            index,
        } => {
            ensure_fresh_id(model, &visual.id)?;
            let p = page_mut(model, &page)?;
            let id = visual.id.clone();
            insert_at(&mut p.visuals, index, visual);
            Ok(vec![Change::RemoveVisual { page, visual: id }])
        }
        Change::RemoveVisual { page, visual } => {
            let p = page_mut(model, &page)?;
            let i = index_of(
                &p.visuals,
                |v| v.id == visual,
                || ChangeError::UnknownVisual { id: visual.clone() },
            )?;
            let v = p.visuals.remove(i);
            Ok(vec![Change::AddVisual {
                page,
                visual: v,
                index: Some(i),
            }])
        }
        Change::UpdateVisual { page, visual } => {
            let p = page_mut(model, &page)?;
            let id = visual.id.clone();
            let v = p
                .visuals
                .iter_mut()
                .find(|v| v.id == id)
                .ok_or(ChangeError::UnknownVisual { id })?;
            let old = std::mem::replace(v, visual);
            Ok(vec![Change::UpdateVisual { page, visual: old }])
        }
        Change::SetReportFilters { filters } => {
            let old = std::mem::replace(&mut model.report.filters, filters);
            Ok(vec![Change::SetReportFilters { filters: old }])
        }
        Change::SetDiagram { positions } => {
            let old = std::mem::replace(&mut model.report.diagram, positions);
            Ok(vec![Change::SetDiagram { positions: old }])
        }
        Change::SetTheme { theme } => {
            let old = std::mem::replace(&mut model.report.theme, theme);
            Ok(vec![Change::SetTheme { theme: old }])
        }
        Change::SetBookmarks { bookmarks } => {
            let old = std::mem::replace(&mut model.report.bookmarks, bookmarks);
            Ok(vec![Change::SetBookmarks { bookmarks: old }])
        }
    }
}
