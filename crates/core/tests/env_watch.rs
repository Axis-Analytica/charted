//! The environment watcher follows theme and language changes the way Omarchy makes them:
//! a theme switch replaces the `current/theme` directory, a language switch rewrites a file.

use charted_core::env::watch::{spawn, EnvChange, EnvPaths};
use std::sync::mpsc;
use std::time::Duration;

fn write_theme(home: &std::path::Path, accent: &str) {
    let current = home.join(".local/state/omarchy/current");
    // Build the new theme beside the old one and swap it in, like `omarchy theme set`.
    let next = current.join("theme.next");
    std::fs::create_dir_all(&next).unwrap();
    std::fs::write(next.join("colors.toml"), format!("accent = \"{accent}\"\nbackground = \"#101010\"\nforeground = \"#eeeeee\"\n")).unwrap();
    let theme = current.join("theme");
    let _ = std::fs::remove_dir_all(&theme);
    std::fs::rename(&next, &theme).unwrap();
}

#[test]
fn follows_theme_switches_and_language_changes() {
    let home = tempfile::tempdir().unwrap();
    write_theme(home.path(), "#111111");
    std::fs::create_dir_all(home.path().join(".config/omarchy")).unwrap();
    let paths = EnvPaths { home: home.path().to_path_buf(), omarchy: home.path().join("no-omarchy") };
    assert_eq!(paths.theme().colors["accent"], "#111111");

    let (tx, rx) = mpsc::channel();
    let _handle = spawn(paths, move |c| {
        let _ = tx.send(c);
    });
    std::thread::sleep(Duration::from_millis(300));

    // Two consecutive switches: the second proves the watcher re-armed after the first.
    for accent in ["#222222", "#333333"] {
        write_theme(home.path(), accent);
        let got = loop {
            match rx.recv_timeout(Duration::from_secs(5)).expect("theme change reported") {
                EnvChange::Theme(t) => break t,
                _ => continue,
            }
        };
        assert_eq!(got.colors["accent"], accent);
    }

    std::fs::write(home.path().join(".config/omarchy/language"), "da\n").unwrap();
    let lang = loop {
        match rx.recv_timeout(Duration::from_secs(5)).expect("language change reported") {
            EnvChange::Language(l) => break l,
            _ => continue,
        }
    };
    assert_eq!(lang.code, "da");
}

#[test]
fn raw_notify_sees_renames() {
    use notify::{RecursiveMode, Watcher};
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = mpsc::channel();
    let mut w = notify::recommended_watcher(move |r: notify::Result<notify::Event>| {
        let _ = tx.send(format!("{r:?}"));
    })
    .unwrap();
    w.watch(dir.path(), RecursiveMode::NonRecursive).unwrap();
    std::fs::create_dir(dir.path().join("x")).unwrap();
    let ev = rx.recv_timeout(Duration::from_secs(3));
    assert!(ev.is_ok(), "no raw event");
}
