// Prevents an extra console window on Windows release builds; harmless elsewhere.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod pdf;
mod pick;
mod screenshot;

use charted_agent::host::{AgentRequest, AgentStatus, Host, Push};
use charted_core::env::EnvSnapshot;
use charted_core::env::watch::{self, EnvPaths};
use charted_core::{ApiError, App, AppState, DataRequest, Request};
use clap::Parser;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::ipc::Response;
use tauri::{Emitter, Manager, WindowEvent};

struct Shared {
    app: Arc<Mutex<App>>,
    /// The in-app agent (started once the window exists, as it pushes to it).
    agent: Mutex<Option<Arc<Host>>>,
    initial: Mutex<Option<String>>,
    env: EnvPaths,
}

fn lock(app: &Arc<Mutex<App>>) -> std::sync::MutexGuard<'_, App> {
    app.lock().unwrap_or_else(|p| p.into_inner())
}

/// Runs work that may touch DuckDB or the disk off the UI thread.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, ApiError> + Send + 'static,
) -> Result<T, ApiError> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| ApiError {
            code: "internal".into(),
            message: e.to_string(),
            detail: None,
        })?
}

#[tauri::command]
async fn rpc(state: tauri::State<'_, Shared>, request: Request) -> Result<AppState, ApiError> {
    let app = state.app.clone();
    blocking(move || lock(&app).handle(request)).await
}

#[tauri::command]
async fn data(state: tauri::State<'_, Shared>, request: DataRequest) -> Result<Response, ApiError> {
    let app = state.app.clone();
    blocking(move || {
        // Resolve under the lock, query without it, so pages load in parallel.
        let job = lock(&app).data_job(request)?;
        job.run().map(Response::new)
    })
    .await
}

#[tauri::command]
async fn agent(
    state: tauri::State<'_, Shared>,
    request: AgentRequest,
) -> Result<AgentStatus, ApiError> {
    let host = state
        .agent
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone();
    let Some(host) = host else {
        return Err(ApiError {
            code: "agent".into(),
            message: "the agent couldn't start".into(),
            detail: None,
        });
    };
    blocking(move || host.handle(request).map_err(charted_agent::host::api_error)).await
}

#[tauri::command]
fn env(state: tauri::State<'_, Shared>) -> EnvSnapshot {
    EnvSnapshot::read(&state.env)
}

#[tauri::command]
fn initial_path(state: tauri::State<'_, Shared>) -> Option<String> {
    state
        .initial
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
}

#[tauri::command]
async fn pick(
    app: tauri::AppHandle,
    options: pick::PickOptions,
) -> Result<Option<String>, ApiError> {
    pick::pick(app, options).await.map_err(|message| ApiError {
        code: "portal".into(),
        message,
        detail: None,
    })
}

/// Opens a new GitHub issue for charted in the browser (a crash report the user
/// reads and sends themselves). No other page can be opened.
#[tauri::command]
fn open_issue(url: String) -> Result<(), ApiError> {
    if !charted_core::crash::is_project_page(&url) {
        return Err(ApiError {
            code: "invalid".into(),
            message: "only charted's GitHub issues and discussions can be opened".into(),
            detail: None,
        });
    }
    open_url(&url).map_err(|e| ApiError {
        code: "invalid".into(),
        message: format!("couldn't open the browser: {e}"),
        detail: None,
    })
}

#[cfg(target_os = "linux")]
fn open_url(url: &str) -> Result<(), String> {
    std::process::Command::new("xdg-open")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(not(target_os = "linux"))]
fn open_url(url: &str) -> Result<(), String> {
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
fn quit(handle: tauri::AppHandle) {
    handle.exit(0);
}

/// Spike command used by the WebKitGTK performance page (`?spike`).
#[tauri::command]
async fn spike_arrow(rows: u64) -> Result<Response, String> {
    // The WebKitGTK performance spike (docs/perf.md); not part of release builds.
    if !cfg!(debug_assertions) {
        return Err("not available in release builds".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let conn = duckdb::Connection::open_in_memory().map_err(|e| e.to_string())?;
        let sql = format!(
            "SELECT range AS id, DATE '2022-01-01' + CAST(range / 700 AS INTEGER) AS order_date, \
             (range % 40)::INTEGER AS store, 'Product ' || (range % 800) AS product, \
             (1 + range % 5)::INTEGER AS quantity, (10 + (range * 7919) % 300)::DOUBLE AS amount \
             FROM range({rows})"
        );
        charted_engine::query_arrow_ipc(&conn, &sql)
            .map(Response::new)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn state_dir() -> PathBuf {
    charted_core::os::state_dir()
}

fn gui(path: Option<PathBuf>, screenshot: Option<PathBuf>, export_pdf: Option<PathBuf>) {
    // The Wayland app id and X11 class come from the program name; it must match the
    // .desktop file so Hyprland and the launcher associate the window with charted.
    #[cfg(target_os = "linux")]
    glib::set_prgname(Some("charted"));
    #[cfg(target_os = "macos")]
    login_path();
    let initial = path.map(|p| {
        charted_core::os::canonicalize(&p)
            .unwrap_or(p)
            .display()
            .to_string()
    });
    charted_core::crash::install_hook(state_dir());
    let app = Arc::new(Mutex::new(App::new(state_dir())));
    let shared = Shared {
        app: app.clone(),
        agent: Mutex::new(None),
        initial: Mutex::new(initial),
        env: EnvPaths::from_env(),
    };

    let builder = tauri::Builder::default();
    // Linux uses the desktop portal and xdg-open instead.
    #[cfg(not(target_os = "linux"))]
    let builder = builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init());
    builder
        .manage(shared)
        .invoke_handler(tauri::generate_handler![
            rpc,
            data,
            agent,
            env,
            initial_path,
            pick,
            quit,
            open_issue,
            spike_arrow,
            pdf::print_pdf,
            pdf::print_dialog,
            pdf::exit_with
        ])
        .setup(move |tauri_app| {
            let handle = tauri_app.handle().clone();
            let paths = EnvPaths::from_env();
            let watch_paths = paths.clone();
            let watcher = watch::spawn(watch_paths, move |_change| {
                let _ = handle.emit("env-changed", EnvSnapshot::read(&paths));
            });
            // Keep the watcher alive for the app's lifetime.
            tauri_app.manage(watcher);
            let handle = tauri_app.handle().clone();
            let app = tauri_app.state::<Shared>().app.clone();
            match Host::start(app, move |push| {
                let _ = match push {
                    Push::State(s) => handle.emit("state-changed", s),
                    Push::Agent(a) => handle.emit("agent-event", a),
                };
            }) {
                Ok(host) => {
                    *tauri_app
                        .state::<Shared>()
                        .agent
                        .lock()
                        .unwrap_or_else(|p| p.into_inner()) = Some(host)
                }
                Err(e) => eprintln!("charted: the agent's socket couldn't start: {e}"),
            }
            if let Some(file) = screenshot.clone() {
                screenshot::schedule(tauri_app.handle().clone(), file);
            }
            if let Some(file) = export_pdf.clone() {
                pdf::schedule(tauri_app.handle().clone(), file);
            }
            Ok(())
        })
        .on_window_event(move |window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event
                && lock(&app).is_dirty()
            {
                api.prevent_close();
                let _ = window.emit("close-requested", ());
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building charted")
        .run(|app_handle, event| {
            // macOS hands files opened from the Finder to the running app, not as
            // arguments; the UI opens the next one it takes from initial_path.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Opened { urls } = &event
                && let Some(path) = urls.iter().find_map(|u| u.to_file_path().ok())
            {
                *app_handle
                    .state::<Shared>()
                    .initial
                    .lock()
                    .unwrap_or_else(|p| p.into_inner()) = Some(path.display().to_string());
                let _ = app_handle.emit("open-file", ());
            }
            // Let queries still running on other threads finish (or stop) before the
            // process exits and DuckDB's static data goes away.
            if let tauri::RunEvent::Exit = event {
                if let Some(host) = app_handle
                    .state::<Shared>()
                    .agent
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .take()
                {
                    host.shutdown();
                }
                charted_core::api::shutdown(std::time::Duration::from_secs(3));
            }
        });
}

/// Apps opened from the Finder get launchd's short PATH, without Homebrew or
/// `~/.local/bin` where the agents are installed. Take the one an interactive login
/// shell has (installers add to `~/.zshrc`, which a non-interactive shell skips), and
/// add the usual install folders in case the shell's setup fails or is slow.
#[cfg(target_os = "macos")]
fn login_path() {
    use std::io::Read;
    use std::time::{Duration, Instant};
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let mut from_shell = None;
    // Markers, since an interactive shell's setup may print its own text.
    if let Ok(mut child) = std::process::Command::new(&shell)
        .args(["-l", "-i", "-c", "printf '__PATH__%s__PATH__' \"$PATH\""])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(3) {
            if let Ok(Some(_)) = child.try_wait() {
                let mut out = String::new();
                if let Some(mut s) = child.stdout.take() {
                    let _ = s.read_to_string(&mut out);
                }
                from_shell = out.split("__PATH__").nth(1).map(str::to_string);
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = child.kill();
    }
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    for p in [from_shell, std::env::var("PATH").ok()]
        .into_iter()
        .flatten()
    {
        dirs.extend(std::env::split_paths(&p));
    }
    if let Some(home) = charted_core::os::home_dir() {
        for d in [
            ".local/bin",
            ".claude/local",
            ".npm-global/bin",
            ".bun/bin",
            ".volta/bin",
            ".cargo/bin",
        ] {
            dirs.push(home.join(d));
        }
    }
    dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(std::path::PathBuf::from));
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|d| !d.as_os_str().is_empty() && seen.insert(d.clone()));
    if let Ok(path) = std::env::join_paths(dirs) {
        // SAFETY: still single-threaded, before Tauri starts.
        unsafe { std::env::set_var("PATH", path) };
    }
}

fn main() {
    let cli = charted_cli::Cli::parse();
    match cli.command {
        None => gui(None, None, None),
        Some(charted_cli::Command::Open {
            path,
            screenshot,
            export_pdf,
        }) => gui(path, screenshot, export_pdf),
        Some(cmd) => std::process::exit(charted_cli::run(cmd)),
    }
}
