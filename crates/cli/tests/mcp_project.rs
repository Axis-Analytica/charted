//! `charted mcp --project DIR`: the agent tools on a project with no window, as an agent
//! run from a script would use them.

use charted_core::api::{App, Request};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

struct Server {
    child: std::process::Child,
    out: std::io::Lines<BufReader<std::process::ChildStdout>>,
    next: u64,
}

impl Server {
    fn start(project: &std::path::Path, extra: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_charted-headless"))
            .args(["mcp", "--project", &project.display().to_string()])
            .args(extra)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let out = BufReader::new(child.stdout.take().unwrap()).lines();
        Self {
            child,
            out,
            next: 0,
        }
    }
    fn call(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(
            stdin,
            "{}",
            json!({ "jsonrpc": "2.0", "id": self.next, "method": method, "params": params })
        )
        .unwrap();
        let line = self.out.next().unwrap().unwrap();
        serde_json::from_str(&line).unwrap()
    }
    fn tool(&mut self, name: &str, args: Value) -> (String, bool) {
        let r = self.call("tools/call", json!({ "name": name, "arguments": args }));
        (
            r["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .to_string(),
            r["result"]["isError"] == true,
        )
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn an_agent_edits_a_project_with_no_window_and_each_change_is_saved() {
    let dir = tempfile::tempdir().unwrap();
    let project = {
        let mut app = App::new(dir.path().join("state"));
        app.handle(Request::OpenSample {
            dir: Some(dir.path().join("p").display().to_string()),
        })
        .unwrap();
        app.session().unwrap().project.dir.clone()
        // Dropping the app releases the project.
    };
    let mut s = Server::start(&project, &[]);
    let init = s.call("initialize", json!({ "protocolVersion": "2025-06-18" }));
    assert_eq!(init["result"]["serverInfo"]["name"], "charted");
    let names: Vec<String> = s.call("tools/list", json!({}))["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert!(names.contains(&"add_visual".to_string()));
    for gone in ["screenshot_page", "draft_query", "sample_rows"] {
        assert!(!names.contains(&gone.to_string()), "{gone} is offered");
    }

    let (text, err) = s.tool("page", json!({ "action": "add", "name": "From a script" }));
    assert!(!err, "{text}");
    let (text, err) = s.tool(
        "add_visual",
        json!({ "page": "From a script", "type": "card", "fields": { "values": ["[Total Sales]"] } }),
    );
    assert!(!err, "{text}");
    // Deletes that would need the user's OK are refused without --allow-deletes.
    let (_, err) = s.tool(
        "page",
        json!({ "action": "remove", "page": "From a script" }),
    );
    assert!(err);

    // The window can't open it meanwhile.
    let mut other = App::new(dir.path().join("state2"));
    let locked = other.handle(Request::OpenProject {
        path: project.display().to_string(),
    });
    assert!(locked.is_err());
    drop(s);

    // Saved: it's there when the project opens again.
    let state = other
        .handle(Request::OpenProject {
            path: project.display().to_string(),
        })
        .unwrap();
    let m = state.project.unwrap().model;
    let p = m
        .report
        .pages
        .iter()
        .find(|p| p.settings.name == "From a script")
        .expect("the page was saved");
    assert_eq!(p.visuals.len(), 1);
}
