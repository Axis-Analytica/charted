//! `charted mcp`: an MCP server over stdio for charted's agent tools.
//!
//! - `--socket` (what the Agent pane starts): it holds no logic, passing tool lists and
//!   calls to the running charted window over its control socket, which decides what the
//!   agent may do.
//! - `--project DIR`: the tools on a project with no window open, for agents run elsewhere
//!   (scripts, CI). It holds the project's lock (so it refuses a project the app has
//!   open), saves after every change, and has no screenshots or Power Query drafts.

use charted_agent::tools::{self, Turn};
use charted_core::api::{App, AppState, Request};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::Mutex;

const PROTOCOLS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];

/// Answers MCP over stdio; `tools` answers tools/list and tools/call (None: gone).
fn run(mut tools: impl FnMut(&str, &Value) -> Option<Value>) -> i32 {
    let stdout = std::io::stdout();
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(id) = msg.get("id").cloned() else {
            continue;
        }; // a notification
        let method = msg["method"].as_str().unwrap_or("");
        let reply = match method {
            "initialize" => {
                let asked = msg["params"]["protocolVersion"].as_str().unwrap_or("");
                let version = if PROTOCOLS.contains(&asked) {
                    asked
                } else {
                    "2025-06-18"
                };
                json!({ "result": {
                    "protocolVersion": version,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "charted", "version": env!("CARGO_PKG_VERSION") }
                } })
            }
            "ping" => json!({ "result": {} }),
            "tools/list" | "tools/call" => match tools(method, &msg["params"]) {
                Some(result) => json!({ "result": result }),
                None => {
                    json!({ "error": { "code": -32000, "message": "charted closed the connection" } })
                }
            },
            _ => {
                json!({ "error": { "code": -32601, "message": format!("unknown method {method}") } })
            }
        };
        let mut out = reply;
        out["jsonrpc"] = json!("2.0");
        out["id"] = id;
        let mut lock = stdout.lock();
        if writeln!(lock, "{out}").and_then(|_| lock.flush()).is_err() {
            break;
        }
    }
    0
}

/// Passes the tools through to the running window.
pub fn serve(socket: &Path, token: &str) -> i32 {
    let conn = match UnixStream::connect(socket) {
        Ok(c) => c,
        Err(e) => return super::fail(format!("can't reach charted at {}: {e}", socket.display())),
    };
    let mut to_app = conn.try_clone().expect("a socket clones");
    let mut from_app = BufReader::new(conn).lines();
    if writeln!(to_app, "{}", json!({ "hello": token })).is_err() {
        return super::fail("charted closed the connection");
    }
    match from_app
        .next()
        .and_then(Result::ok)
        .and_then(|l| serde_json::from_str::<Value>(&l).ok())
    {
        Some(v) if v["ok"] == true => {}
        Some(v) => return super::fail(v["error"].as_str().unwrap_or("charted refused the agent")),
        None => return super::fail("charted closed the connection"),
    }
    let mut next = 0u64;
    run(|method, params| {
        next += 1;
        writeln!(
            to_app,
            "{}",
            json!({ "id": next, "method": method, "params": params })
        )
        .ok()?;
        let v: Value = serde_json::from_str(&from_app.next()?.ok()?).ok()?;
        Some(v["result"].clone())
    })
}

/// The tools a windowless agent gets: no screenshots (nothing draws pages) and no Power
/// Query drafts (nobody is there to apply them).
const WINDOW_ONLY: &[&str] = &["screenshot_page", "draft_query"];

struct Headless {
    group: String,
    data: bool,
    deletes: bool,
}

impl Turn for Headless {
    fn group(&self) -> &str {
        &self.group
    }
    fn label(&self) -> &str {
        "Agent (charted mcp)"
    }
    fn data(&self) -> bool {
        self.data
    }
    /// Nobody to ask: deletes that need an OK happen only with --allow-deletes.
    fn confirm(&self, _what: &str) -> bool {
        self.deletes
    }
    fn changed(&self, _: &AppState) {}
    fn draft(&self) -> Option<Vec<charted_compiler::steps::Query>> {
        None
    }
    fn set_draft(&self, _: Option<Vec<charted_compiler::steps::Query>>) {}
}

/// Serves the tools on a project directly, saving after each change.
pub fn serve_project(project: &Path, data: bool, deletes: bool) -> i32 {
    let state = std::env::temp_dir().join(format!("charted-mcp-{}", std::process::id()));
    let mut app = App::new(state.clone());
    if let Err(e) = app.handle(Request::OpenProject {
        path: project.display().to_string(),
    }) {
        let _ = std::fs::remove_dir_all(&state);
        return super::fail(format!("can't open {}: {}", project.display(), e.message));
    }
    let app = Mutex::new(app);
    let turn = Headless {
        group: uuid::Uuid::new_v4().to_string(),
        data,
        deletes,
    };
    let code = run(|method, params| {
        Some(if method == "tools/list" {
            let list: Vec<Value> = tools::list()
                .into_iter()
                .filter(|t| !WINDOW_ONLY.contains(&t.name) && (data || !tools::DATA_TOOLS.contains(&t.name)))
                .map(|t| json!({ "name": t.name, "description": t.description, "inputSchema": t.input_schema }))
                .collect();
            json!({ "tools": list })
        } else {
            let name = params["name"].as_str().unwrap_or("");
            let args = match &params["arguments"] {
                Value::Null => json!({}),
                a => a.clone(),
            };
            let (text, is_error) = if WINDOW_ONLY.contains(&name) {
                (
                    format!(
                        "{name} needs the charted window; open the project in charted and use its Agent pane"
                    ),
                    true,
                )
            } else {
                let o = tools::call(&app, &turn, name, &args);
                // A change: written to the project at once (no one else will save it).
                let mut text = o.text;
                if !o.is_error && o.step.is_some() {
                    let mut a = app.lock().unwrap_or_else(|p| p.into_inner());
                    let dirty = a.session().is_some_and(|s| s.is_dirty());
                    if dirty && let Err(e) = a.handle(Request::Save) {
                        text.push_str(&format!(" (Not saved: {}.)", e.message));
                    }
                }
                (text, o.is_error)
            };
            json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
        })
    });
    drop(app);
    let _ = std::fs::remove_dir_all(&state);
    code
}
