//! Crash reports: a panic writes `crash-<time>.log` (the message, where, a backtrace
//! and the version) to charted's state folder. The next start offers to report it — a
//! GitHub issue the user reads and sends themselves; nothing is sent automatically.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use ts_rs::TS;

/// The last crash, not yet dismissed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CrashInfo {
    pub path: String,
    /// Seconds since the epoch.
    #[ts(type = "number")]
    pub at: u64,
    /// The first line of the report (the panic message).
    pub message: String,
    /// A GitHub "new issue" link with the report filled in.
    pub issue_url: String,
}

const REPO: &str = "https://github.com/Axis-Analytica/charted";

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Writes a crash log for every panic (then lets the default hook print it).
pub fn install_hook(state_dir: PathBuf) {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let at = now();
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panic".into());
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let text = format!(
            "{message}\n\nat {location}\nthread {}\ncharted {} on {} {}\n\n{}\n",
            std::thread::current().name().unwrap_or("?"),
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            std::backtrace::Backtrace::force_capture()
        );
        let _ = std::fs::create_dir_all(&state_dir);
        let _ = std::fs::write(state_dir.join(format!("crash-{at}.log")), text);
        default(info);
    }));
}

fn percent(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The newest crash log that hasn't been dismissed.
pub fn pending(state_dir: &Path) -> Option<CrashInfo> {
    let mut logs: Vec<(u64, PathBuf)> = std::fs::read_dir(state_dir)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let at = name
                .strip_prefix("crash-")?
                .strip_suffix(".log")?
                .parse()
                .ok()?;
            Some((at, e.path()))
        })
        .collect();
    logs.sort();
    let (at, path) = logs.pop()?;
    let text = std::fs::read_to_string(&path).ok()?;
    let message = text
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(200)
        .collect::<String>();
    // The issue: the report up to the backtrace's first frames (URLs have limits).
    let body: String = text.chars().take(4000).collect();
    let issue_url = format!(
        "{REPO}/issues/new?title={}&body={}",
        percent(&format!("Crash: {message}")),
        percent(&format!(
            "charted closed unexpectedly.\n\nWhat I was doing:\n\n\n```\n{body}\n```\n"
        ))
    );
    Some(CrashInfo {
        path: path.display().to_string(),
        at,
        message,
        issue_url,
    })
}

/// Marks crash logs as seen (renames them; the files are kept for reference).
pub fn dismiss(state_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(state_dir) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if let Some(rest) = name.strip_prefix("crash-") {
            let _ = std::fs::rename(e.path(), state_dir.join(format!("seen-crash-{rest}")));
        }
    }
}

/// Whether `url` is a new GitHub issue for charted (the only page charted opens).
pub fn is_issue_url(url: &str) -> bool {
    url.starts_with(&format!("{REPO}/issues/new?"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_logs_become_issue_links_until_dismissed() {
        let dir = tempfile::tempdir().unwrap();
        assert!(pending(dir.path()).is_none());
        std::fs::write(dir.path().join("crash-100.log"), "old\n").unwrap();
        std::fs::write(
            dir.path().join("crash-200.log"),
            "index out of bounds: the len is 3\n\nat src/x.rs:1\n",
        )
        .unwrap();
        let c = pending(dir.path()).unwrap();
        assert_eq!(
            (c.at, c.message.as_str()),
            (200, "index out of bounds: the len is 3")
        );
        assert!(is_issue_url(&c.issue_url));
        assert!(c.issue_url.contains("Crash%3A%20index%20out%20of%20bounds"));
        dismiss(dir.path());
        assert!(pending(dir.path()).is_none());
        assert!(dir.path().join("seen-crash-200.log").exists());
        assert!(!is_issue_url("https://example.com/issues/new?x"));
    }
}
