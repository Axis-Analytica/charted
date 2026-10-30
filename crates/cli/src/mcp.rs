//! `charted mcp`: an MCP server over stdio for the agent the Agent pane starts. It holds
//! no logic: it answers the MCP handshake and passes tool lists and calls to the running
//! charted window over its control socket, which decides what the agent may do.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;

const PROTOCOLS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];

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
    let stdout = std::io::stdout();
    let mut next = 0u64;
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
            "tools/list" | "tools/call" => {
                next += 1;
                let sent = writeln!(
                    to_app,
                    "{}",
                    json!({ "id": next, "method": method, "params": msg["params"] })
                );
                match sent
                    .ok()
                    .and_then(|_| from_app.next())
                    .and_then(Result::ok)
                    .and_then(|l| serde_json::from_str::<Value>(&l).ok())
                {
                    Some(v) => json!({ "result": v["result"] }),
                    None => {
                        json!({ "error": { "code": -32000, "message": "charted closed the connection" } })
                    }
                }
            }
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
