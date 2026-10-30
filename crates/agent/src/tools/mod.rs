//! The tools the agent builds with. Each takes JSON arguments, reads or changes the open
//! project through the `App` (every change one commit in the request's undo group), and
//! answers in text for the agent plus a [`Step`] for the Agent pane. They run in the app's
//! own process, so what the agent may see (`Turn::data`) can't be got round.

mod model;
mod query;
mod read;
mod report;

use charted_core::api::{App, AppState};
use charted_model::{Change, Model};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Mutex;
use ts_rs::TS;

/// The request a tool call belongs to.
pub trait Turn: Sync {
    /// The undo group every change joins.
    fn group(&self) -> &str;
    /// The undo label ("Agent: …").
    fn label(&self) -> &str;
    /// The user lets the agent see data (rows, values).
    fn data(&self) -> bool;
    /// Asks the user to allow `what` (a big delete); blocks until they answer.
    fn confirm(&self, what: &str) -> bool;
    /// A change landed: the window shows it.
    fn changed(&self, state: &AppState);
    /// The queries as the agent has drafted them, if it has.
    fn draft(&self) -> Option<Vec<charted_compiler::steps::Query>>;
    fn set_draft(&self, queries: Option<Vec<charted_compiler::steps::Query>>);
}

/// One thing the agent did, for the Agent pane: `text` is English, and `op` with `args`
/// is the same as a message the UI translates (ui/src/agent/stepText.ts).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "AgentStep")]
pub struct Step {
    pub tool: String,
    pub ok: bool,
    pub op: String,
    pub args: Vec<String>,
    pub text: String,
    /// The page it happened on (the canvas follows it).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub page: Option<String>,
    /// The visuals it touched.
    #[serde(default)]
    pub visuals: Vec<String>,
    /// DAX or fields, shown when the step is expanded.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub detail: Option<String>,
}

impl Step {
    pub fn new(op: &str, args: &[&str]) -> Self {
        let mut text = op.to_string();
        for (i, a) in args.iter().enumerate() {
            text = text.replace(&format!("%{}", i + 1), a);
        }
        Self {
            tool: String::new(),
            ok: true,
            op: op.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
            text,
            page: None,
            visuals: vec![],
            detail: None,
        }
    }

    pub fn page(mut self, page: &str) -> Self {
        self.page = Some(page.into());
        self
    }

    pub fn visuals(mut self, ids: Vec<String>) -> Self {
        self.visuals = ids;
        self
    }

    pub fn detail(mut self, d: impl Into<String>) -> Self {
        self.detail = Some(d.into());
        self
    }
}

/// What a tool gives back.
#[derive(Debug, Clone)]
pub struct Outcome {
    /// For the agent.
    pub text: String,
    pub is_error: bool,
    /// For the pane (reads have none).
    pub step: Option<Step>,
}

/// A tool's failure, in words for the agent.
#[derive(Debug, Clone)]
pub struct Fail(pub String);

impl<E: std::fmt::Display> From<E> for Fail {
    fn from(e: E) -> Self {
        Fail(e.to_string())
    }
}

pub type Res<T> = Result<T, Fail>;

pub fn fail<T>(msg: impl Into<String>) -> Res<T> {
    Err(Fail(msg.into()))
}

/// What a tool is, for `tools/list`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDef {
    pub name: &'static str,
    pub description: String,
    pub input_schema: Value,
}

pub fn list() -> Vec<ToolDef> {
    let mut v = read::defs();
    v.extend(report::defs());
    v.extend(model::defs());
    v.extend(query::defs());
    v
}

/// The tools only for a user who lets the agent see data.
pub const DATA_TOOLS: &[&str] = &["sample_rows", "distinct_values"];

/// Runs a tool.
pub fn call(app: &Mutex<App>, turn: &dyn Turn, name: &str, args: &Value) -> Outcome {
    let cx = Cx { app, turn };
    if DATA_TOOLS.contains(&name) && !turn.data() {
        return Outcome {
            text: format!(
                "{name} needs data access, which is off: the user hasn't let the agent see rows or values. Work from the model's names and the visuals' totals, or ask the user to turn on \"Let the agent see data\" in the Agent pane."
            ),
            is_error: true,
            step: Some(Step {
                tool: name.into(),
                ok: false,
                ..Step::new("Needs data access: %1", &[name])
            }),
        };
    }
    let result = read::call(&cx, name, args)
        .or_else(|| report::call(&cx, name, args))
        .or_else(|| model::call(&cx, name, args))
        .or_else(|| query::call(&cx, name, args))
        .unwrap_or_else(|| fail(format!("there's no tool {name}")));
    match result {
        Ok(Done { text, step }) => Outcome {
            text,
            is_error: false,
            step: step.map(|s| Step {
                tool: name.into(),
                ..s
            }),
        },
        Err(Fail(msg)) => Outcome {
            step: Some(Step {
                tool: name.into(),
                ok: false,
                ..Step::new("Not done: %1", &[&short(&msg)])
            }),
            text: msg,
            is_error: true,
        },
    }
}

/// The first sentence of a message, for a step's line.
fn short(msg: &str) -> String {
    let first = msg.split("; ").next().unwrap_or(msg);
    let mut s: String = first.chars().take(140).collect();
    if first.chars().count() > 140 {
        s.push('…');
    }
    s
}

/// A tool's answer: text for the agent and, for a change, a step.
pub struct Done {
    pub text: String,
    pub step: Option<Step>,
}

impl Done {
    pub fn read(text: impl Into<String>) -> Res<Self> {
        Ok(Self {
            text: text.into(),
            step: None,
        })
    }
}

/// A tool's context.
pub struct Cx<'a> {
    pub app: &'a Mutex<App>,
    pub turn: &'a dyn Turn,
}

impl Cx<'_> {
    pub fn lock(&self) -> std::sync::MutexGuard<'_, App> {
        self.app.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Reads the open project's model.
    pub fn with_model<T>(
        &self,
        f: impl FnOnce(&Model, &charted_core::session::Session) -> Res<T>,
    ) -> Res<T> {
        let app = self.lock();
        let s = app
            .session()
            .ok_or_else(|| Fail("no project is open".into()))?;
        f(&s.project.model, s)
    }

    /// Builds changes from the current model and commits them in the request's group.
    /// `build` returns the changes and what to tell the agent and the pane.
    pub fn change(
        &self,
        build: impl FnOnce(&Model, &charted_core::session::Session) -> Res<(Vec<Change>, Done)>,
    ) -> Res<Done> {
        let state = {
            let mut app = self.lock();
            let s = app
                .session()
                .ok_or_else(|| Fail("no project is open".into()))?;
            let (changes, done) = build(&s.project.model, s)?;
            if changes.is_empty() {
                return Ok(done);
            }
            let state = app
                .commit_grouped(self.turn.label(), changes, self.turn.group())
                .map_err(|e| Fail(e.message))?;
            (state, done)
        };
        self.turn.changed(&state.0);
        Ok(state.1)
    }
}

/// Argument helpers.
pub fn str_arg<'a>(args: &'a Value, key: &str) -> Res<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| Fail(format!("\"{key}\" is required (text)")))
}

pub fn opt_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
}

pub fn opt_bool(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(Value::as_bool)
}

pub fn str_list(args: &Value, key: &str) -> Vec<String> {
    match args.get(key) {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => vec![],
    }
}

pub fn action<'a>(args: &'a Value, allowed: &[&str]) -> Res<&'a str> {
    let a = str_arg(args, "action")?;
    if allowed.contains(&a) {
        Ok(a)
    } else {
        fail(format!("action is one of {}", allowed.join(", ")))
    }
}

/// A JSON schema for an object.
pub fn schema(props: Value, required: &[&str]) -> Value {
    serde_json::json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
}

/// DAX diagnostics as text, or None when there are none.
pub fn diagnostics(src: &str, ds: &[charted_dax::Diagnostic]) -> Option<String> {
    if ds.is_empty() {
        return None;
    }
    Some(
        ds.iter()
            .map(|d| {
                let at = src.get(d.span.start..d.span.end).unwrap_or("");
                if at.is_empty() {
                    d.message.clone()
                } else {
                    format!("{} (at \"{at}\")", d.message)
                }
            })
            .collect::<Vec<_>>()
            .join("; "),
    )
}
