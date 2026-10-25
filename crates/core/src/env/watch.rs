//! Live watching of the desktop environment.
//!
//! Theme switches replace files and symlinks under `~/.local/state/omarchy/current`, so
//! the watcher listens on the stable parent directories and re-reads on any event, and
//! re-arms after each change. Hyprland settings changed with `hyprctl keyword` emit no
//! event, so they are polled through Hyprland's request socket (no process spawn).

use notify::{RecursiveMode, Watcher};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use super::desktop::{Desktop, read_desktop};
use super::language::{Language, language_file, locales_file, read_language};
use super::theme::{Theme, read_theme, theme_dir};

#[derive(Debug, Clone)]
pub enum EnvChange {
    Theme(Theme),
    Desktop(Desktop),
    Language(Language),
}

#[derive(Debug, Clone)]
pub struct EnvPaths {
    pub home: PathBuf,
    pub omarchy: PathBuf,
}

impl EnvPaths {
    pub fn from_env() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        let omarchy = std::env::var_os("OMARCHY_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/usr/share/omarchy"));
        Self { home, omarchy }
    }

    pub fn theme(&self) -> Theme {
        read_theme(&theme_dir(&self.home))
    }

    pub fn language(&self) -> Language {
        read_language(&language_file(&self.home), &locales_file(&self.omarchy))
    }
}

/// Stops the watcher threads when dropped.
pub struct WatchHandle {
    stop: Arc<AtomicBool>,
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn hypr_request(cmd: &str) -> Option<serde_json::Value> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")?;
    let sig = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    let path = Path::new(&runtime)
        .join("hypr")
        .join(sig)
        .join(".socket.sock");
    let mut s = UnixStream::connect(path).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(500))).ok()?;
    s.write_all(cmd.as_bytes()).ok()?;
    let mut buf = String::new();
    s.read_to_string(&mut buf).ok()?;
    serde_json::from_str(&buf).ok()
}

/// Reads Hyprland settings over its socket, falling back to `hyprctl`.
pub fn desktop_now() -> Desktop {
    let rounding = hypr_request("j/getoption decoration:rounding");
    let animations = hypr_request("j/getoption animations:enabled");
    match (rounding, animations) {
        (Some(r), Some(a)) => Desktop {
            hyprland: true,
            rounding: r.get("int").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            animations: a
                .get("bool")
                .and_then(|b| b.as_bool())
                .or_else(|| a.get("int").and_then(|i| i.as_i64()).map(|i| i != 0))
                .unwrap_or(true),
        },
        _ => read_desktop(),
    }
}

/// Starts watching and calls `on_change` from a background thread whenever something
/// changes. Initial values are not reported; read them with `EnvPaths`.
pub fn spawn(
    paths: EnvPaths,
    on_change: impl Fn(EnvChange) + Send + Sync + 'static,
) -> WatchHandle {
    let stop = Arc::new(AtomicBool::new(false));
    let on_change = Arc::new(on_change);

    // File events: debounce, re-read, report differences, re-arm.
    {
        let stop = stop.clone();
        let on_change = on_change.clone();
        let paths = paths.clone();
        std::thread::Builder::new()
            .name("charted-env-files".into())
            .spawn(move || {
                let (tx, rx) = mpsc::channel::<()>();
                let Ok(mut watcher) =
                    notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                        // Reading the theme opens its files; ignore access events or every
                        // read would trigger another.
                        if matches!(&res, Ok(e) if !matches!(e.kind, notify::EventKind::Access(_)))
                        {
                            let _ = tx.send(());
                        }
                    })
                else {
                    tracing::warn!(
                        "file watching is unavailable; theme and language changes need a restart"
                    );
                    return;
                };
                let current = paths.home.join(".local/state/omarchy/current");
                let config = paths.home.join(".config/omarchy");
                // The parent directories are stable, so they are watched once. The theme
                // directory is replaced on every switch, so only it is re-armed:
                // re-watching the stable ones races with inotify's removal notices and
                // silently loses later events.
                for dir in [&current, &config] {
                    if dir.exists() {
                        let _ = watcher.watch(dir, RecursiveMode::NonRecursive);
                    }
                }
                let theme_dir = current.join("theme");
                let arm = |w: &mut notify::RecommendedWatcher| {
                    let _ = w.unwatch(&theme_dir);
                    if theme_dir.exists() {
                        let _ = w.watch(&theme_dir, RecursiveMode::NonRecursive);
                    }
                };
                arm(&mut watcher);
                let mut theme = paths.theme();
                let mut language = paths.language();
                while !stop.load(Ordering::Relaxed) {
                    match rx.recv_timeout(Duration::from_millis(500)) {
                        Ok(()) => {
                            std::thread::sleep(Duration::from_millis(150));
                            while rx.try_recv().is_ok() {}
                            let t = paths.theme();
                            if t != theme {
                                theme = t.clone();
                                on_change(EnvChange::Theme(t));
                            }
                            let l = paths.language();
                            if l != language {
                                language = l.clone();
                                on_change(EnvChange::Language(l));
                            }
                            // A theme switch replaces watched directories and symlinks.
                            arm(&mut watcher);
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            })
            .expect("spawn env watcher");
    }

    // Hyprland: poll cheaply over the socket.
    {
        let stop = stop.clone();
        std::thread::Builder::new()
            .name("charted-env-hypr".into())
            .spawn(move || {
                let mut last = desktop_now();
                if !last.hyprland {
                    return;
                }
                while !stop.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(1500));
                    let d = desktop_now();
                    if d != last {
                        last = d.clone();
                        on_change(EnvChange::Desktop(d));
                    }
                }
            })
            .expect("spawn hyprland watcher");
    }

    WatchHandle { stop }
}
