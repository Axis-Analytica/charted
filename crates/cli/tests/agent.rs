//! The in-app agent end to end, without a window: the host serves its socket, a stand-in
//! agent (ui/e2e/fake-agent.mjs, run as Claude Code would be) starts `charted mcp` and
//! builds a page through it.

use charted_agent::host::{AgentEvent, AgentRequest, Host, Push};
use charted_agent::transcript::{Item, TurnStatus};
use charted_core::api::{App, Request};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn node() -> bool {
    std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_ok()
}

#[test]
fn a_stand_in_agent_builds_a_page_through_the_socket() {
    if !node() {
        eprintln!("skipped: no node");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    // SAFETY: set before any thread of this test reads the environment.
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", dir.path().join("run"));
        std::env::set_var("CHARTED_AGENT_MCP", env!("CARGO_BIN_EXE_charted-headless"));
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../../ui/e2e/fake-agent.mjs");
        std::env::set_var("CHARTED_AGENT_CMD", format!("node {script}"));
    }
    let app = Arc::new(Mutex::new(App::new(dir.path().join("state"))));
    app.lock()
        .unwrap()
        .handle(Request::OpenSample {
            dir: Some(dir.path().join("p").display().to_string()),
        })
        .unwrap();
    let pushed = Arc::new(Mutex::new((0usize, None)));
    let seen = pushed.clone();
    let host = Host::start(app.clone(), move |p| {
        let mut s = seen.lock().unwrap();
        match p {
            Push::State(_) => s.0 += 1,
            Push::Agent(AgentEvent::Turn { turn }) => s.1 = Some(turn),
            Push::Agent(_) => {}
        }
    })
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(host.socket())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let send = || AgentRequest::Send {
        text: "build me a page".into(),
        page: None,
        visuals: vec![],
    };
    host.handle(AgentRequest::SetAgent {
        agent: "claude".into(),
        model: String::new(),
    })
    .unwrap();
    assert_eq!(host.handle(send()).unwrap_err(), "consent");
    host.handle(AgentRequest::Consent).unwrap();
    host.handle(send()).unwrap();
    let started = Instant::now();
    while host.status().running {
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "the agent didn't finish"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let status = host.status();
    let turn = status.turns.last().unwrap();
    assert_eq!(turn.status, TurnStatus::Done, "{:?}", turn.items);
    assert_eq!(turn.cost_usd, Some(0.0123));
    let steps: Vec<_> = turn
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Step { step } => Some(step.text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(steps.len(), 5, "{steps:?}");
    assert!(steps[1].contains("Sales by month"), "{steps:?}");
    assert!(matches!(turn.items.last(), Some(Item::Text { text }) if text.contains("Agent e2e")));
    assert!(pushed.lock().unwrap().0 >= 5);
    // One undo entry for the request, and the follow-up resumes the session.
    let mut a = app.lock().unwrap();
    let state = a.handle(Request::State).unwrap();
    let project = state.project.unwrap();
    assert_eq!(
        project.undo_label.as_deref(),
        Some("Agent: build me a page")
    );
    assert_eq!(project.undo_group.as_deref(), Some(turn.group.as_str()));
    assert!(
        project
            .model
            .report
            .pages
            .iter()
            .any(|p| p.settings.name == "Agent e2e")
    );
    a.handle(Request::UndoGroup {
        group: turn.group.clone(),
    })
    .unwrap();
    assert!(
        !a.session()
            .unwrap()
            .project
            .model
            .report
            .pages
            .iter()
            .any(|p| p.settings.name == "Agent e2e")
    );
    drop(a);
    // The conversation is kept with the project.
    let chat = std::fs::read_to_string(dir.path().join("p/.cache/agent/chat.json")).unwrap();
    assert!(
        chat.contains("build me a page") && chat.contains("fake-session")
            || chat.contains("\"claude\"")
    );
    host.shutdown();
    assert!(!host.socket().exists());
}
