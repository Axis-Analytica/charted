//! The order calculated tables and calculated columns are computed in.
//!
//! A calculated table can read calculated columns of other tables and a calculated
//! column can read a calculated table, so both are nodes of one dependency graph. Edges
//! come from the names an expression mentions (measures expanded recursively). That
//! is read from the parsed expression rather than the bound one so that every function,
//! including ones the compiler doesn't support yet, contributes its references; a name
//! that could mean several things counts as all of them.

use std::collections::{BTreeMap, BTreeSet};

use charted_dax::{Expr, ExprKind};
use charted_model::{ColumnKind, Id, Model, Table};

/// A calculated table or calculated column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Table { table: Id },
    Column { table: Id, column: Id },
}

impl Item {
    /// The table or column id (ids are unique across the model).
    pub fn id(&self) -> &str {
        match self {
            Item::Table { table } => table,
            Item::Column { column, .. } => column,
        }
    }

    pub fn table(&self) -> &str {
        match self {
            Item::Table { table } | Item::Column { table, .. } => table,
        }
    }
}

pub struct Graph {
    /// Every calculated item, in the order to compute them (dependencies first).
    pub order: Vec<Item>,
    /// What each item reads, by id: other items and the text of measures it uses.
    pub deps: BTreeMap<Id, BTreeSet<Id>>,
    pub measures: BTreeMap<Id, Vec<String>>,
    /// Items that depend on themselves, with the names of the items in their cycle.
    pub cycles: BTreeMap<Id, Vec<String>>,
}

/// The items of a model: calculated tables, then calculated columns, in model order.
pub fn items(model: &Model) -> Vec<Item> {
    let mut out = Vec::new();
    for t in &model.tables {
        if t.calculated.is_some() {
            out.push(Item::Table {
                table: t.id.clone(),
            });
        }
    }
    for t in &model.tables {
        for c in &t.columns {
            if matches!(c.kind, ColumnKind::Calculated { .. }) {
                out.push(Item::Column {
                    table: t.id.clone(),
                    column: c.id.clone(),
                });
            }
        }
    }
    out
}

/// A readable name: `'Table'` or `Table[Column]`.
pub fn item_name(model: &Model, item: &Item) -> String {
    let t = model.table(item.table());
    let tname = t.map(|t| t.name.as_str()).unwrap_or("?");
    match item {
        Item::Table { .. } => format!("'{tname}'"),
        Item::Column { column, .. } => {
            let c = t.and_then(|t| t.column(column)).map(|c| c.name.as_str());
            format!("{tname}[{}]", c.unwrap_or("?"))
        }
    }
}

pub fn build(model: &Model) -> Graph {
    let all = items(model);
    let mut deps = BTreeMap::new();
    let mut measures = BTreeMap::new();
    for item in &all {
        let mut r = Refs::default();
        match item {
            Item::Table { table } => {
                if let Some(calc) = model.table(table).and_then(|t| t.calculated.as_ref()) {
                    r.expression(model, &calc.expression, None, true);
                }
            }
            Item::Column { table, column } => {
                let t = model.table(table).expect("item table");
                if let Some(ColumnKind::Calculated { expression, .. }) =
                    t.column(column).map(|c| &c.kind)
                {
                    r.expression(model, expression, Some(t), false);
                }
                // A calculated table's columns are added to its rows once they exist.
                if t.calculated.is_some() {
                    r.items.insert(table.clone());
                }
            }
        }
        deps.insert(item.id().to_string(), r.items);
        measures.insert(item.id().to_string(), r.measure_text);
    }
    // Depth-first, dependencies first; a dependency met again while still open closes
    // a cycle.
    let by_id: BTreeMap<&str, &Item> = all.iter().map(|i| (i.id(), i)).collect();
    let mut order = Vec::new();
    let mut state: BTreeMap<&str, u8> = BTreeMap::new();
    fn visit<'a>(
        id: &'a str,
        by_id: &BTreeMap<&'a str, &'a Item>,
        deps: &'a BTreeMap<Id, BTreeSet<Id>>,
        state: &mut BTreeMap<&'a str, u8>,
        order: &mut Vec<Item>,
    ) {
        if state.contains_key(id) {
            return;
        }
        state.insert(id, 1);
        for d in deps.get(id).into_iter().flatten() {
            if by_id.contains_key(d.as_str()) {
                visit(d, by_id, deps, state, order);
            }
        }
        state.insert(id, 2);
        order.push(by_id[id].clone());
    }
    for item in &all {
        visit(item.id(), &by_id, &deps, &mut state, &mut order);
    }
    // An item is in a cycle when it can reach itself.
    let mut cycles = BTreeMap::new();
    for item in &all {
        let reach = reachable(item.id(), &deps);
        if reach.contains(item.id()) {
            let members: Vec<String> = all
                .iter()
                .filter(|o| reach.contains(o.id()) && reachable(o.id(), &deps).contains(item.id()))
                .map(|o| item_name(model, o))
                .collect();
            cycles.insert(item.id().to_string(), members);
        }
    }
    Graph {
        order,
        deps,
        measures,
        cycles,
    }
}

fn reachable(from: &str, deps: &BTreeMap<Id, BTreeSet<Id>>) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut stack: Vec<&str> = deps
        .get(from)
        .into_iter()
        .flatten()
        .map(|s| s.as_str())
        .collect();
    while let Some(id) = stack.pop() {
        if seen.insert(id.to_string()) {
            stack.extend(deps.get(id).into_iter().flatten().map(|s| s.as_str()));
        }
    }
    seen
}

/// The message for an item in a cycle.
pub fn cycle_message(members: &[String]) -> String {
    format!(
        "a circular dependency was detected: {}",
        members
            .iter()
            .chain(members.first())
            .cloned()
            .collect::<Vec<_>>()
            .join(" → ")
    )
}

#[derive(Default)]
struct Refs {
    items: BTreeSet<Id>,
    seen_measures: BTreeSet<Id>,
    measure_text: Vec<String>,
}

impl Refs {
    /// `whole_tables`: a table named on its own brings its calculated columns along
    /// (a calculated table's rows carry every column of the table they come from).
    fn expression(&mut self, model: &Model, src: &str, row: Option<&Table>, whole_tables: bool) {
        let parsed = charted_dax::parse(src);
        self.walk(model, &parsed.expr, row, whole_tables);
    }

    fn walk(&mut self, model: &Model, e: &Expr, row: Option<&Table>, whole: bool) {
        match &e.kind {
            ExprKind::Ref {
                table: Some(tname),
                name,
            } => {
                if let Some(t) = model.table_by_name(tname) {
                    self.table(t, false);
                    if let Some(c) = t.column_by_name(name) {
                        self.column(t, c);
                    }
                    if let Some(m) = t
                        .measures
                        .iter()
                        .find(|m| m.name.eq_ignore_ascii_case(name))
                    {
                        self.measure(model, &m.id, &m.expression);
                    }
                }
            }
            ExprKind::Ref { table: None, name } => {
                if let Some(t) = row
                    && let Some(c) = t.column_by_name(name)
                {
                    self.column(t, c);
                }
                for t in &model.tables {
                    if let Some(m) = t
                        .measures
                        .iter()
                        .find(|m| m.name.eq_ignore_ascii_case(name))
                    {
                        self.measure(model, &m.id, &m.expression);
                    }
                }
            }
            ExprKind::Table(name) => {
                if let Some(t) = model.table_by_name(name) {
                    self.table(t, whole);
                }
            }
            ExprKind::Call { args, .. } | ExprKind::List(args) => {
                args.iter().for_each(|a| self.walk(model, a, row, whole))
            }
            ExprKind::Unary { expr, .. } => self.walk(model, expr, row, whole),
            ExprKind::Binary { left, right, .. } => {
                self.walk(model, left, row, whole);
                self.walk(model, right, row, whole);
            }
            ExprKind::Var { value, body, .. } => {
                self.walk(model, value, row, whole);
                self.walk(model, body, row, whole);
            }
            _ => {}
        }
    }

    fn table(&mut self, t: &Table, whole: bool) {
        if t.calculated.is_some() {
            self.items.insert(t.id.clone());
        }
        if whole {
            for c in &t.columns {
                if matches!(c.kind, ColumnKind::Calculated { .. }) {
                    self.items.insert(c.id.clone());
                }
            }
        }
    }

    fn column(&mut self, t: &Table, c: &charted_model::Column) {
        if t.calculated.is_some() {
            self.items.insert(t.id.clone());
        }
        if matches!(c.kind, ColumnKind::Calculated { .. }) {
            self.items.insert(c.id.clone());
        }
    }

    /// A measure is read where it's used: what it reads counts for the user.
    fn measure(&mut self, model: &Model, id: &str, expression: &str) {
        if !self.seen_measures.insert(id.to_string()) {
            return;
        }
        self.measure_text.push(expression.to_string());
        let parsed = charted_dax::parse(expression);
        self.walk(model, &parsed.expr, None, false);
    }
}
