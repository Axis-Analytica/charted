//! Power Query, as drafts: the agent changes a copy of the queries, each step checked by
//! working out the columns it leaves, and the user applies the draft from the Agent pane
//! (applying reloads data and clears undo, so it is never the agent's to do).

use charted_compiler::steps::Query;
use charted_core::DataRequest;
use serde_json::{Value, json};
use std::fmt::Write;

use super::{Cx, Done, Fail, Res, Step, ToolDef, action, fail, opt_str, schema, str_arg};

pub fn defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "describe_queries",
            description: "The Power Query queries that load the tables: each one's source and steps (as JSON, the form draft_query takes). Includes your draft's changes when there is one.".into(),
            input_schema: schema(json!({ "query": { "type": "string", "description": "One query, with the columns it gives" } }), &[]),
        },
        ToolDef {
            name: "draft_query",
            description: "Changes a query's steps in a draft the user applies (applying reloads the data; you can't apply it). action: add_step (at index, default the end), replace_step, remove_step, or discard (the whole draft). step is a step as describe_queries shows them, e.g. {\"op\": \"removeColumns\", \"columns\": [\"Notes\"]}, {\"op\": \"renameColumns\", \"renames\": [[\"Old\", \"New\"]]}, {\"op\": \"filterRows\", …}, {\"op\": \"changeTypes\", …}, {\"op\": \"customColumn\", …}. Each change is checked; the answer lists the query's columns after it.".into(),
            input_schema: schema(
                json!({
                    "action": { "type": "string", "enum": ["add_step", "replace_step", "remove_step", "discard"] },
                    "query": { "type": "string" },
                    "index": { "type": "integer", "minimum": 0 },
                    "step": { "type": "object" }
                }),
                &["action"],
            ),
        },
    ]
}

pub fn call(cx: &Cx, name: &str, args: &Value) -> Option<Res<Done>> {
    Some(match name {
        "describe_queries" => describe(cx, args),
        "draft_query" => draft(cx, args),
        _ => return None,
    })
}

/// The queries as drafted (or as they are).
fn current(cx: &Cx) -> Res<Vec<Query>> {
    match cx.turn.draft() {
        Some(d) => Ok(d),
        None => cx.with_model(|_, s| Ok(s.project.queries.clone())),
    }
}

fn find<'a>(queries: &'a [Query], name: &str) -> Res<&'a Query> {
    queries
        .iter()
        .find(|q| q.name.eq_ignore_ascii_case(name.trim()))
        .ok_or_else(|| {
            Fail(format!(
                "there's no query \"{name}\"; there are {}",
                queries
                    .iter()
                    .map(|q| format!("\"{}\"", q.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })
}

/// A query's columns after its last step (which also checks its steps).
fn columns(cx: &Cx, q: &Query, all: &[Query]) -> Res<String> {
    let job = cx
        .lock()
        .data_job(DataRequest::QueryInfo {
            query: q.clone(),
            queries: all.to_vec(),
            upto: q.steps.len(),
        })
        .map_err(|e| Fail(e.message))?;
    let info: Value = serde_json::from_slice(&job.run().map_err(|e| Fail(e.message))?)?;
    if let Some(e) = info.get("error").and_then(Value::as_str) {
        return fail(e.to_string());
    }
    let cols = info["columns"]
        .as_array()
        .map(|cs| {
            cs.iter()
                .map(|c| {
                    format!(
                        "{} {}",
                        c["name"].as_str().unwrap_or("?"),
                        c["dataType"].as_str().unwrap_or("")
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    Ok(cols)
}

fn describe(cx: &Cx, args: &Value) -> Res<Done> {
    let queries = current(cx)?;
    let mut out = String::new();
    if cx.turn.draft().is_some() {
        out.push_str("(with your draft's changes, not applied yet)\n");
    }
    let shown: Vec<&Query> = match opt_str(args, "query") {
        Some(n) => vec![find(&queries, n)?],
        None => queries.iter().collect(),
    };
    for q in shown {
        let source = serde_json::to_value(&q.source)?;
        let _ = writeln!(
            out,
            "query \"{}\" → table {}{}: source {} {}",
            q.name,
            q.table,
            if q.no_load { " (not loaded)" } else { "" },
            source["kind"].as_str().unwrap_or("?"),
            q.source.path()
        );
        for (i, s) in q.steps.iter().enumerate() {
            let _ = writeln!(out, "  {i}. {}", serde_json::to_string(s)?);
        }
        if opt_str(args, "query").is_some() {
            let _ = writeln!(out, "  columns: {}", columns(cx, q, &queries)?);
        }
    }
    Done::read(out)
}

fn draft(cx: &Cx, args: &Value) -> Res<Done> {
    let act = action(
        args,
        &["add_step", "replace_step", "remove_step", "discard"],
    )?;
    if act == "discard" {
        cx.turn.set_draft(None);
        return Ok(Done {
            text: "Discarded the draft.".into(),
            step: Some(Step::new("Discarded the query draft", &[])),
        });
    }
    let mut queries = current(cx)?;
    let name = find(&queries, str_arg(args, "query")?)?.name.clone();
    let at = queries.iter().position(|q| q.name == name).expect("found");
    let q = &mut queries[at];
    let index = args
        .get("index")
        .and_then(Value::as_u64)
        .map(|i| i as usize);
    let parse = || -> Res<charted_compiler::steps::Step> {
        serde_json::from_value(
            args.get("step")
                .cloned()
                .ok_or_else(|| Fail("step is required".into()))?,
        )
        .map_err(|e| Fail(format!("step: {e}")))
    };
    let (op, what) = match act {
        "add_step" => {
            let s = parse()?;
            let i = index.unwrap_or(q.steps.len()).min(q.steps.len());
            let what = serde_json::to_string(&s)?;
            q.steps.insert(i, s);
            ("Drafted a step in query \"%1\"", what)
        }
        "replace_step" => {
            let i = index.ok_or_else(|| Fail("replace_step needs index".into()))?;
            let s = parse()?;
            let what = serde_json::to_string(&s)?;
            let n = q.steps.len();
            *q.steps
                .get_mut(i)
                .ok_or_else(|| Fail(format!("\"{name}\" has {n} steps")))? = s;
            ("Drafted a change to query \"%1\"", what)
        }
        _ => {
            let i = index.ok_or_else(|| Fail("remove_step needs index".into()))?;
            if i >= q.steps.len() {
                return fail(format!("\"{name}\" has {} steps", q.steps.len()));
            }
            let s = q.steps.remove(i);
            (
                "Drafted removing a step from query \"%1\"",
                serde_json::to_string(&s)?,
            )
        }
    };
    let q = queries[at].clone();
    let cols = columns(cx, &q, &queries)?;
    cx.turn.set_draft(Some(queries));
    Ok(Done {
        text: format!(
            "Drafted. \"{name}\" now gives: {cols}. The user applies the draft from the Agent pane."
        ),
        step: Some(Step::new(op, &[&name]).detail(what)),
    })
}
