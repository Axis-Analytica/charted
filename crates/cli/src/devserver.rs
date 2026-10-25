//! A development server exposing `App::handle` over a WebSocket on 127.0.0.1, so the UI
//! can run in an ordinary browser (Playwright end-to-end tests, screenshots). It speaks
//! the protocol of ui/src/bridge/ws.ts: JSON text frames `{id, method, params}` answered
//! with `{id, ok}` or `{id, err}`; data replies are binary frames carrying an 8-byte
//! little-endian request id followed by Arrow IPC bytes.

use charted_core::env::watch::EnvPaths;
use charted_core::env::EnvSnapshot;
use charted_core::{ApiError, App};
use serde_json::{json, Value};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tungstenite::handshake::server::{Request as HsRequest, Response as HsResponse};
use tungstenite::Message;

pub fn serve(port: u16, token: &str, state_dir: Option<PathBuf>) -> std::io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let state_dir = state_dir.unwrap_or_else(|| std::env::temp_dir().join(format!("charted-dev-{}", std::process::id())));
    let app = Arc::new(Mutex::new(App::new(state_dir)));
    eprintln!("charted dev server on ws://127.0.0.1:{port}");
    for stream in listener.incoming() {
        let stream = stream?;
        let app = app.clone();
        let token = token.to_string();
        std::thread::spawn(move || {
            let check = |req: &HsRequest, resp: HsResponse| {
                let ok = req.uri().query().unwrap_or("").split('&').any(|kv| kv == format!("token={token}"));
                if ok || token.is_empty() {
                    Ok(resp)
                } else {
                    let mut r = tungstenite::handshake::server::ErrorResponse::new(Some("invalid token".into()));
                    *r.status_mut() = tungstenite::http::StatusCode::FORBIDDEN;
                    Err(r)
                }
            };
            let Ok(mut ws) = tungstenite::accept_hdr(stream, check) else { return };
            let paths = EnvPaths::from_env();
            loop {
                let msg = match ws.read() {
                    Ok(Message::Text(t)) => t,
                    Ok(Message::Close(_)) | Err(_) => break,
                    Ok(_) => continue,
                };
                let Ok(req) = serde_json::from_str::<Value>(&msg) else { continue };
                let id = req["id"].as_u64().unwrap_or(0);
                let params = req["params"].clone();
                let lock = || app.lock().unwrap_or_else(|p| p.into_inner());
                let reply = match req["method"].as_str().unwrap_or("") {
                    "rpc" => match serde_json::from_value(params) {
                        Ok(r) => lock().handle(r).map(|s| serde_json::to_value(s).unwrap()),
                        Err(e) => Err(ApiError { code: "invalid".into(), message: e.to_string() }),
                    },
                    "data" => {
                        let result = serde_json::from_value(params)
                            .map_err(|e| ApiError { code: "invalid".into(), message: e.to_string() })
                            .and_then(|r| lock().data(r));
                        match result {
                            Ok(bytes) => {
                                let mut frame = id.to_le_bytes().to_vec();
                                frame.extend_from_slice(&bytes);
                                if ws.send(Message::Binary(frame.into())).is_err() {
                                    break;
                                }
                                continue;
                            }
                            Err(e) => Err(e),
                        }
                    }
                    "env" => Ok(serde_json::to_value(EnvSnapshot::read(&paths)).unwrap()),
                    "pick" => Ok(std::env::var("CHARTED_PICK_RESULT").map(Value::String).unwrap_or(Value::Null)),
                    "quit" => Ok(Value::Null),
                    other => Err(ApiError { code: "invalid".into(), message: format!("unknown method {other}") }),
                };
                let out = match reply {
                    Ok(v) => json!({ "id": id, "ok": v }),
                    Err(e) => json!({ "id": id, "err": e }),
                };
                if ws.send(Message::Text(out.to_string().into())).is_err() {
                    break;
                }
            }
        });
    }
    Ok(())
}
