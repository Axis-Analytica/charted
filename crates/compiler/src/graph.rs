//! Which tables filter which: paths along active relationships from a table on the
//! many side to the tables on the one side (one-to-one relationships work both ways).
//! Filters on a reachable table restrict rows of the starting table, as in Power BI's
//! single-direction cross-filtering.

use charted_model::{Cardinality, Id, Model, Relationship};
use std::collections::{BTreeMap, VecDeque};

/// One hop: join `to` from `from` on `from_column = to_column`.
#[derive(Debug, Clone, PartialEq)]
pub struct Hop {
    pub from: Id,
    pub from_column: Id,
    pub to: Id,
    pub to_column: Id,
}

#[derive(Debug, Clone, Default)]
pub struct Reach {
    /// For each reachable table (not the start), the hop that reaches it.
    via: BTreeMap<Id, Hop>,
    /// Tables reachable along more than one path.
    ambiguous: Vec<Id>,
}

fn hops(r: &Relationship) -> Vec<Hop> {
    let forward = Hop {
        from: r.from_table.clone(),
        from_column: r.from_column.clone(),
        to: r.to_table.clone(),
        to_column: r.to_column.clone(),
    };
    let backward = Hop {
        from: r.to_table.clone(),
        from_column: r.to_column.clone(),
        to: r.from_table.clone(),
        to_column: r.from_column.clone(),
    };
    match (r.from_cardinality, r.to_cardinality) {
        (Cardinality::Many, Cardinality::One) => vec![forward],
        (Cardinality::One, Cardinality::Many) => vec![backward],
        (Cardinality::One, Cardinality::One) => vec![forward, backward],
        // Many-to-many relationships do not propagate filters here.
        (Cardinality::Many, Cardinality::Many) => vec![],
    }
}

impl Reach {
    /// Breadth-first from `start` along active relationships.
    pub fn from(model: &Model, start: &str) -> Self {
        Self::with(model, start, &[])
    }

    /// Like `from`, with relationships switched on by USERELATIONSHIP: each replaces the
    /// active relationship between the same two tables.
    pub fn with(model: &Model, start: &str, use_rels: &[Id]) -> Self {
        let on = |r: &Relationship| {
            if use_rels.contains(&r.id) {
                return true;
            }
            let overridden = use_rels.iter().any(|u| {
                u != &r.id
                    && model.relationship(u).is_some_and(|o| {
                        (o.from_table == r.from_table && o.to_table == r.to_table)
                            || (o.from_table == r.to_table && o.to_table == r.from_table)
                    })
            });
            r.active && !overridden
        };
        let edges: Vec<Hop> = model
            .relationships
            .iter()
            .filter(|r| on(r))
            .flat_map(hops)
            .collect();
        let mut reach = Reach::default();
        let mut queue = VecDeque::from([start.to_string()]);
        while let Some(t) = queue.pop_front() {
            for h in edges.iter().filter(|h| h.from == t) {
                if h.to == start {
                    continue;
                }
                if reach.via.contains_key(&h.to) {
                    if reach.via[&h.to] != *h && !reach.ambiguous.contains(&h.to) {
                        reach.ambiguous.push(h.to.clone());
                    }
                    continue;
                }
                reach.via.insert(h.to.clone(), h.clone());
                queue.push_back(h.to.clone());
            }
        }
        reach
    }

    pub fn reaches(&self, start: &str, table: &str) -> bool {
        table == start || self.via.contains_key(table)
    }

    pub fn is_ambiguous(&self, table: &str) -> bool {
        self.ambiguous.iter().any(|t| t == table)
    }

    /// The hops from the start to `table`, in join order.
    pub fn path(&self, table: &str) -> Vec<Hop> {
        let mut out = Vec::new();
        let mut t = table.to_string();
        while let Some(h) = self.via.get(&t) {
            out.push(h.clone());
            t = h.from.clone();
        }
        out.reverse();
        out
    }
}
