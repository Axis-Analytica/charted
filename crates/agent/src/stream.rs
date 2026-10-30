//! What the agents print as they work (Claude Code's `stream-json`, Codex's `--json`), read
//! into the few things the Agent pane shows.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The agent's session, for follow-ups.
    Session(String),
    /// Something it said.
    Text(String),
    /// A tool it may use that isn't charted's: the turn stops (it could read files).
    Forbidden(String),
    Cost(f64),
    /// The turn ended.
    Finished { ok: bool, message: Option<String> },
}

pub fn claude(line: &str) -> Vec<Event> {
    let Ok(v) = serde_json::from_str::<Value>(line) else { return vec![] };
    let mut out = vec![];
    match (v["type"].as_str(), v["subtype"].as_str()) {
        (Some("system"), Some("init")) => {
            if let Some(s) = v["session_id"].as_str() {
                out.push(Event::Session(s.into()));
            }
            for t in v["tools"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                if !t.starts_with("mcp__charted__") {
                    out.push(Event::Forbidden(t.into()));
                }
            }
        }
        (Some("assistant"), _) => {
            for b in v["message"]["content"].as_array().into_iter().flatten() {
                if b["type"] == "text"
                    && let Some(t) = b["text"].as_str().filter(|t| !t.trim().is_empty())
                {
                    out.push(Event::Text(t.trim().into()));
                }
            }
        }
        (Some("result"), sub) => {
            if let Some(s) = v["session_id"].as_str() {
                out.push(Event::Session(s.into()));
            }
            if let Some(c) = v["total_cost_usd"].as_f64() {
                out.push(Event::Cost(c));
            }
            let ok = !v["is_error"].as_bool().unwrap_or(false) && sub == Some("success");
            let message = (!ok).then(|| match sub {
                Some("error_max_turns") => "The agent took too many steps and stopped.".to_string(),
                _ => v["result"].as_str().map(str::to_string).unwrap_or_else(|| sub.unwrap_or("error").to_string()),
            });
            out.push(Event::Finished { ok, message });
        }
        _ => {}
    }
    out
}

pub fn codex(line: &str) -> Vec<Event> {
    let Ok(v) = serde_json::from_str::<Value>(line) else { return vec![] };
    match v["type"].as_str() {
        Some("thread.started") => v["thread_id"].as_str().map(|s| vec![Event::Session(s.into())]).unwrap_or_default(),
        Some("item.completed") | Some("item.started") => {
            let item = &v["item"];
            match item["type"].as_str() {
                Some("agent_message") if v["type"] == "item.completed" => item["text"]
                    .as_str()
                    .filter(|t| !t.trim().is_empty())
                    .map(|t| vec![Event::Text(t.trim().into())])
                    .unwrap_or_default(),
                Some(t @ ("command_execution" | "file_change" | "web_search")) => vec![Event::Forbidden(t.into())],
                _ => vec![],
            }
        }
        Some("turn.completed") => vec![Event::Finished { ok: true, message: None }],
        Some("turn.failed") => vec![Event::Finished {
            ok: false,
            message: Some(v["error"]["message"].as_str().unwrap_or("the agent failed").into()),
        }],
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_code_is_read() {
        let init = r#"{"type":"system","subtype":"init","session_id":"s1","tools":["mcp__charted__add_visual"]}"#;
        assert_eq!(claude(init), vec![Event::Session("s1".into())]);
        let bad = r#"{"type":"system","subtype":"init","session_id":"s1","tools":["Bash","mcp__charted__x"]}"#;
        assert!(claude(bad).contains(&Event::Forbidden("Bash".into())));
        let said = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Done."},{"type":"tool_use","name":"mcp__charted__page"}]}}"#;
        assert_eq!(claude(said), vec![Event::Text("Done.".into())]);
        let end = r#"{"type":"result","subtype":"success","is_error":false,"result":"hi","session_id":"s1","total_cost_usd":0.02}"#;
        assert_eq!(claude(end), vec![Event::Session("s1".into()), Event::Cost(0.02), Event::Finished { ok: true, message: None }]);
        let over = r#"{"type":"result","subtype":"error_max_turns","is_error":true}"#;
        assert!(matches!(&claude(over)[0], Event::Finished { ok: false, message: Some(m) } if m.contains("too many")));
    }

    #[test]
    fn codex_is_read() {
        assert_eq!(codex(r#"{"type":"thread.started","thread_id":"t"}"#), vec![Event::Session("t".into())]);
        assert_eq!(
            codex(r#"{"type":"item.completed","item":{"type":"agent_message","text":"ok"}}"#),
            vec![Event::Text("ok".into())]
        );
        assert_eq!(
            codex(r#"{"type":"item.started","item":{"type":"command_execution","command":"ls"}}"#),
            vec![Event::Forbidden("command_execution".into())]
        );
        assert!(matches!(&codex(r#"{"type":"turn.failed","error":{"message":"401"}}"#)[0], Event::Finished { ok: false, .. }));
    }
}
