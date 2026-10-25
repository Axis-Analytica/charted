//! A development server exposing `App::handle` over a WebSocket on 127.0.0.1, so the UI
//! can run in an ordinary browser (Playwright end-to-end tests, screenshots). It speaks
//! the protocol of ui/src/bridge/ws.ts: JSON text frames `{id, method, params}` answered
//! with `{id, ok}` or `{id, err}`; data replies are binary frames carrying an 8-byte
//! little-endian request id followed by Arrow IPC bytes.

use charted_core::env::EnvSnapshot;
use charted_core::env::watch::EnvPaths;
use charted_core::{ApiError, App};
use serde_json::{Value, json};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tungstenite::Message;
use tungstenite::handshake::server::{Request as HsRequest, Response as HsResponse};

pub fn serve(port: u16, token: &str, state_dir: Option<PathBuf>) -> std::io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let state_dir = state_dir.unwrap_or_else(|| {
        std::env::temp_dir().join(format!("charted-dev-{}", std::process::id()))
    });
    let app = Arc::new(Mutex::new(App::new(state_dir)));
    eprintln!("charted dev server on ws://127.0.0.1:{port}");
    for stream in listener.incoming() {
        let stream = stream?;
        let app = app.clone();
        let token = token.to_string();
        std::thread::spawn(move || {
            // The handshake callback's error type is fixed by tungstenite.
            #[allow(clippy::result_large_err)]
            let check = |req: &HsRequest, resp: HsResponse| {
                let ok = req
                    .uri()
                    .query()
                    .unwrap_or("")
                    .split('&')
                    .any(|kv| kv == format!("token={token}"));
                if ok || token.is_empty() {
                    Ok(resp)
                } else {
                    let mut r = tungstenite::handshake::server::ErrorResponse::new(Some(
                        "invalid token".into(),
                    ));
                    *r.status_mut() = tungstenite::http::StatusCode::FORBIDDEN;
                    Err(r)
                }
            };
            let Ok(mut ws) = tungstenite::accept_hdr(stream, check) else {
                return;
            };
            let paths = EnvPaths::from_env();
            // Short read timeouts let one thread both read requests and send replies:
            // data requests run on their own threads (as in the app, where they run in
            // parallel and a newer one can cancel an older one), and desktop theme
            // changes are pushed as `env` events.
            let _ = ws
                .get_ref()
                .set_read_timeout(Some(std::time::Duration::from_millis(20)));
            let (tx, rx) = std::sync::mpsc::channel::<Message>();
            let mut env_seen = serde_json::to_value(EnvSnapshot::read(&paths)).unwrap();
            let mut env_checked = std::time::Instant::now();
            'conn: loop {
                while let Ok(m) = rx.try_recv() {
                    if ws.send(m).is_err() {
                        break 'conn;
                    }
                }
                if env_checked.elapsed() > std::time::Duration::from_millis(400) {
                    env_checked = std::time::Instant::now();
                    let now = serde_json::to_value(EnvSnapshot::read(&paths)).unwrap();
                    if now != env_seen {
                        env_seen = now.clone();
                        let event = json!({ "event": "env", "payload": now });
                        if ws.send(Message::Text(event.to_string().into())).is_err() {
                            break;
                        }
                    }
                }
                let msg = match ws.read() {
                    Ok(Message::Text(t)) => t,
                    Ok(Message::Close(_)) => break,
                    Err(tungstenite::Error::Io(e))
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) =>
                    {
                        continue;
                    }
                    Err(_) => break,
                    Ok(_) => continue,
                };
                let Ok(req) = serde_json::from_str::<Value>(&msg) else {
                    continue;
                };
                let id = req["id"].as_u64().unwrap_or(0);
                let params = req["params"].clone();
                let lock = || app.lock().unwrap_or_else(|p| p.into_inner());
                let error = |id: u64, e: ApiError| {
                    Message::Text(json!({ "id": id, "err": e }).to_string().into())
                };
                let reply = match req["method"].as_str().unwrap_or("") {
                    "rpc" => match serde_json::from_value(params) {
                        Ok(r) => lock().handle(r).map(|s| serde_json::to_value(s).unwrap()),
                        Err(e) => Err(ApiError {
                            code: "invalid".into(),
                            message: e.to_string(),
                            detail: None,
                        }),
                    },
                    "data" => {
                        let job = serde_json::from_value(params)
                            .map_err(|e| ApiError {
                                code: "invalid".into(),
                                message: e.to_string(),
                                detail: None,
                            })
                            .and_then(|r| lock().data_job(r));
                        match job {
                            Ok(job) => {
                                let tx = tx.clone();
                                std::thread::spawn(move || {
                                    let m = match job.run() {
                                        Ok(bytes) => {
                                            let mut frame = id.to_le_bytes().to_vec();
                                            frame.extend_from_slice(&bytes);
                                            Message::Binary(frame.into())
                                        }
                                        Err(e) => error(id, e),
                                    };
                                    let _ = tx.send(m);
                                });
                                continue;
                            }
                            Err(e) => Err(e),
                        }
                    }
                    "env" => Ok(serde_json::to_value(EnvSnapshot::read(&paths)).unwrap()),
                    "pick" => Ok(std::env::var("CHARTED_PICK_RESULT")
                        .map(Value::String)
                        .unwrap_or(Value::Null)),
                    "quit" => Ok(Value::Null),
                    other => Err(ApiError {
                        code: "invalid".into(),
                        message: format!("unknown method {other}"),
                        detail: None,
                    }),
                };
                let out = match reply {
                    Ok(v) => Message::Text(json!({ "id": id, "ok": v }).to_string().into()),
                    Err(e) => error(id, e),
                };
                if ws.send(out).is_err() {
                    break;
                }
            }
        });
    }
    Ok(())
}
