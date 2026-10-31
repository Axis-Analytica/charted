//! The agent's side of the running app. It owns the control socket the agent's tools are
//! served on (`$XDG_RUNTIME_DIR/charted/<pid>.sock`, for this user only), starts the
//! desktop's agent for each request, and keeps the conversation. The desktop shell and the
//! development server each make one and pass on what it pushes to the window.

use charted_core::api::{App, AppState};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, Weak};
use ts_rs::TS;

use crate::settings::{ProjectSettings, UserSettings};
use crate::stream::{self, Event};
use crate::tools::{self, Step, Turn};
use crate::transcript::{Item, Transcript, TurnRecord, TurnStatus};

/// The agent's refusals as API errors; `consent` asks the window for the user's consent.
pub fn api_error(message: String) -> charted_core::ApiError {
    charted_core::ApiError {
        code: if message == "consent" {
            "consent"
        } else {
            "agent"
        }
        .into(),
        message,
        detail: None,
    }
}

/// Steps a request may take before the agent is stopped.
pub const MAX_TURNS: u32 = 40;

/// What the host tells the window.
pub enum Push {
    State(Box<AppState>),
    Agent(AgentEvent),
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum AgentEvent {
    /// A request's record changed (a step, text, a question, the end).
    Turn { turn: TurnRecord },
    /// Everything else changed (agent, data access, a new chat).
    Status { status: AgentStatus },
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentInfo {
    /// The command (claude, codex, opencode …).
    pub name: String,
    pub label: String,
    /// Runs inside the Agent pane (else in a terminal).
    pub headless: bool,
    /// Models to offer ("" is the agent's own default).
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentStatus {
    /// Agents installed.
    pub agents: Vec<AgentInfo>,
    /// The one picked (else Omarchy's default agent).
    #[ts(optional)]
    pub agent: Option<String>,
    pub model: String,
    /// The user agreed to send to the picked agent's provider.
    pub consented: bool,
    /// The agent may see rows and values in this project.
    pub data: bool,
    pub running: bool,
    pub turns: Vec<TurnRecord>,
    /// Queries the agent has drafted changes to, waiting for the user to apply them.
    pub draft: Vec<String>,
    /// The project's instructions for the agent (agent.md).
    pub instructions: String,
}

/// What the window asks of the agent.
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum AgentRequest {
    Status,
    /// A request, with the page and visuals the user is looking at.
    Send {
        text: String,
        #[serde(default)]
        page: Option<String>,
        #[serde(default)]
        visuals: Vec<String>,
    },
    Stop,
    /// An answer to a question (a delete).
    Answer {
        id: String,
        allow: bool,
    },
    NewChat,
    SetData {
        on: bool,
    },
    SetAgent {
        agent: String,
        model: String,
    },
    Consent,
    /// Opens the agent in a terminal with charted's tools (agents that can't run here).
    OpenTerminal {
        text: String,
    },
    /// Applies the agent's query draft: the data reloads, and undo starts afresh.
    ApplyDraft,
    DiscardDraft,
    /// Replaces the project's instructions for the agent (agent.md).
    SetInstructions {
        text: String,
    },
}

const AGENTS: &[(&str, &str, bool)] = &[
    ("claude", "Claude Code", true),
    ("codex", "Codex", true),
    // These open in a terminal (terminal_command): only agents whose MCP set-up from a
    // working directory is known, so they start with charted's tools.
    ("opencode", "OpenCode", false),
    ("cursor-agent", "Cursor CLI", false),
    ("copilot", "GitHub Copilot", false),
    ("gemini", "Gemini CLI", false),
];

/// How a terminal agent starts in charted's working directory with the request, its MCP
/// settings read from there (written by `open_terminal`). No permission-skipping flags:
/// the agent asks in its terminal as usual.
fn terminal_command(agent: &str, prompt: &str) -> Option<Vec<String>> {
    let p = prompt.to_string();
    Some(match agent {
        // opencode.json in the working directory.
        "opencode" => vec!["opencode".into(), "--prompt".into(), p],
        // .cursor/mcp.json.
        "cursor-agent" => vec!["cursor-agent".into(), "agent".into(), "--".into(), p],
        // A workspace .mcp.json loads only once the folder is trusted, so it's named.
        "copilot" => vec![
            "copilot".into(),
            "--additional-mcp-config".into(),
            "@.mcp.json".into(),
            "--interactive".into(),
            p,
        ],
        // .gemini/settings.json.
        "gemini" => vec!["gemini".into(), "--prompt-interactive".into(), p],
        _ => return None,
    })
}

fn on_path(cmd: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file()))
}

/// The agents installed. `CHARTED_AGENT_CMD` (the tests' stand-in) is Claude Code.
pub fn installed() -> Vec<AgentInfo> {
    // With the stand-in, nothing real is offered: a test never starts a real agent.
    let fake = std::env::var_os("CHARTED_AGENT_CMD").is_some();
    AGENTS
        .iter()
        .filter(|(c, _, _)| if fake { *c == "claude" } else { on_path(c) })
        .map(|(c, l, h)| AgentInfo {
            name: c.to_string(),
            label: l.to_string(),
            headless: *h,
            models: match *c {
                "claude" => ["", "opus", "sonnet", "haiku"].map(String::from).to_vec(),
                _ => vec![String::new()],
            },
        })
        .collect()
}

/// Omarchy's default agent, when one is set.
fn omarchy_default() -> Option<String> {
    let out = Command::new("omarchy-default-agent").output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

struct Running {
    turn: String,
    child: Arc<Mutex<Child>>,
    stopped: bool,
}

struct Inner {
    /// Live tokens and the turn each belongs to.
    tokens: HashMap<String, Arc<TurnCtx>>,
    running: Option<Running>,
    /// The project the transcript is for, and it.
    chat: Option<(PathBuf, Transcript)>,
    /// Questions waiting for the user.
    pending: HashMap<String, Sender<bool>>,
    /// The agent's drafted queries, for the project they're for.
    draft: Option<(PathBuf, Vec<charted_compiler::steps::Query>)>,
}

pub struct Host {
    app: Arc<Mutex<App>>,
    emit: Box<dyn Fn(Push) + Send + Sync>,
    socket: PathBuf,
    inner: Mutex<Inner>,
    me: Weak<Host>,
}

/// A request's context for the tools it calls.
pub struct TurnCtx {
    host: Weak<Host>,
    turn: String,
    group: String,
    label: String,
    data: bool,
}

impl Turn for TurnCtx {
    fn group(&self) -> &str {
        &self.group
    }
    fn label(&self) -> &str {
        &self.label
    }
    fn data(&self) -> bool {
        self.data
    }
    fn confirm(&self, what: &str) -> bool {
        let Some(host) = self.host.upgrade() else {
            return false;
        };
        let id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = channel();
        host.lock().pending.insert(id.clone(), tx);
        host.record(
            &self.turn,
            Item::Confirm {
                id: id.clone(),
                what: what.into(),
                answer: None,
            },
        );
        let allow = rx.recv().unwrap_or(false);
        host.update_turn(&self.turn, |t| {
            for item in &mut t.items {
                if let Item::Confirm { id: i, answer, .. } = item
                    && *i == id
                {
                    *answer = Some(allow);
                }
            }
        });
        allow
    }
    fn changed(&self, state: &AppState) {
        if let Some(host) = self.host.upgrade() {
            (host.emit)(Push::State(Box::new(state.clone())));
        }
    }
    fn draft(&self) -> Option<Vec<charted_compiler::steps::Query>> {
        let host = self.host.upgrade()?;
        let dir = host.project_dir()?;
        host.lock()
            .draft
            .as_ref()
            .filter(|(d, _)| *d == dir)
            .map(|(_, q)| q.clone())
    }
    fn set_draft(&self, queries: Option<Vec<charted_compiler::steps::Query>>) {
        let Some(host) = self.host.upgrade() else {
            return;
        };
        let dir = host.project_dir();
        host.lock().draft = dir.zip(queries);
        host.push_status();
    }
}

fn runtime_dir(state_dir: &Path) -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(d) => PathBuf::from(d).join("charted"),
        None => state_dir.join("run"),
    }
}

fn token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    unsafe { libc::kill(pid, 0) == 0 }
}

impl Host {
    /// Starts the control socket. `emit` passes what the host pushes to the window.
    pub fn start(
        app: Arc<Mutex<App>>,
        emit: impl Fn(Push) + Send + Sync + 'static,
    ) -> std::io::Result<Arc<Host>> {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        let state_dir = app
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .state_dir()
            .to_path_buf();
        let dir = runtime_dir(&state_dir);
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)?;
        // Sockets of charted windows that have gone.
        for e in std::fs::read_dir(&dir)?.flatten() {
            let p = e.path();
            if let Some(pid) = p
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.parse::<i32>().ok())
                && p.extension().is_some_and(|x| x == "sock")
                && !alive(pid)
            {
                let _ = std::fs::remove_file(&p);
            }
        }
        let socket = dir.join(format!("{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket)?;
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
        let host = Arc::new_cyclic(|me| Host {
            app,
            emit: Box::new(emit),
            socket,
            inner: Mutex::new(Inner {
                tokens: HashMap::new(),
                running: None,
                chat: None,
                pending: HashMap::new(),
                draft: None,
            }),
            me: me.clone(),
        });
        let weak = Arc::downgrade(&host);
        std::thread::spawn(move || {
            for conn in listener.incoming().flatten() {
                let Some(h) = weak.upgrade() else { break };
                std::thread::spawn(move || h.serve(conn));
            }
        });
        Ok(host)
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Stops the agent and removes the socket (the app is closing).
    pub fn shutdown(&self) {
        self.stop();
        let _ = std::fs::remove_file(&self.socket);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn app(&self) -> std::sync::MutexGuard<'_, App> {
        self.app.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn project_dir(&self) -> Option<PathBuf> {
        self.app().session().map(|s| s.project.dir.clone())
    }

    fn state_dir(&self) -> PathBuf {
        self.app().state_dir().to_path_buf()
    }

    /// Runs `f` on the open project's transcript (loaded when the project changed).
    fn with_chat<T>(&self, f: impl FnOnce(&mut Transcript) -> T) -> Option<T> {
        let dir = self.project_dir()?;
        let mut inner = self.lock();
        if inner.chat.as_ref().is_none_or(|(d, _)| *d != dir) {
            inner.chat = Some((dir.clone(), Transcript::load(&dir)));
        }
        let (d, chat) = inner.chat.as_mut().expect("loaded");
        let out = f(chat);
        chat.save(d);
        Some(out)
    }

    fn update_turn(&self, turn: &str, f: impl FnOnce(&mut TurnRecord)) {
        let record = self
            .with_chat(|c| {
                let t = c.turns.iter_mut().find(|t| t.id == turn)?;
                f(t);
                Some(t.clone())
            })
            .flatten();
        if let Some(turn) = record {
            (self.emit)(Push::Agent(AgentEvent::Turn { turn }));
        }
    }

    fn record(&self, turn: &str, item: Item) {
        self.update_turn(turn, |t| t.items.push(item));
    }

    pub fn status(&self) -> AgentStatus {
        let user = UserSettings::load(&self.state_dir());
        let agents = installed();
        let agent = user
            .agent
            .clone()
            .filter(|a| agents.iter().any(|x| &x.name == a))
            .or_else(|| omarchy_default().filter(|a| agents.iter().any(|x| &x.name == a)))
            .or_else(|| agents.first().map(|a| a.name.clone()));
        let data = self
            .project_dir()
            .is_some_and(|d| ProjectSettings::load(&d).data);
        let running = self.lock().running.is_some();
        let draft = self.drafted();
        let instructions = self
            .project_dir()
            .map(|d| crate::settings::instructions(&d))
            .unwrap_or_default();
        let turns = self.with_chat(|c| c.turns.clone()).unwrap_or_default();
        AgentStatus {
            model: agent
                .as_ref()
                .and_then(|a| user.models.get(a).cloned())
                .unwrap_or_default(),
            consented: agent.as_ref().is_some_and(|a| user.consent.contains_key(a)),
            agent,
            agents,
            data,
            running,
            turns,
            draft,
            instructions,
        }
    }

    /// The names of the queries the draft changes.
    fn drafted(&self) -> Vec<String> {
        let Some(dir) = self.project_dir() else {
            return vec![];
        };
        let draft = match &self.lock().draft {
            Some((d, q)) if *d == dir => q.clone(),
            _ => return vec![],
        };
        let app = self.app();
        let Some(s) = app.session() else {
            return vec![];
        };
        draft
            .iter()
            .filter(|q| s.project.queries.iter().find(|o| o.id == q.id) != Some(*q))
            .map(|q| q.name.clone())
            .collect()
    }

    fn push_status(&self) -> AgentStatus {
        let s = self.status();
        (self.emit)(Push::Agent(AgentEvent::Status { status: s.clone() }));
        s
    }

    /// Handles a request from the window.
    pub fn handle(&self, req: AgentRequest) -> Result<AgentStatus, String> {
        match req {
            AgentRequest::Status => {}
            AgentRequest::Send {
                text,
                page,
                visuals,
            } => self.send(&text, page.as_deref(), &visuals)?,
            AgentRequest::Stop => self.stop(),
            AgentRequest::Answer { id, allow } => {
                if let Some(tx) = self.lock().pending.remove(&id) {
                    let _ = tx.send(allow);
                }
            }
            AgentRequest::NewChat => {
                if self.lock().running.is_some() {
                    return Err("stop the agent first".into());
                }
                self.with_chat(|c| *c = Transcript::default());
            }
            AgentRequest::SetData { on } => {
                let dir = self.project_dir().ok_or("no project is open")?;
                ProjectSettings { data: on }.save(&dir);
            }
            AgentRequest::SetAgent { agent, model } => {
                let sd = self.state_dir();
                let mut u = UserSettings::load(&sd);
                if model.is_empty() {
                    u.models.remove(&agent);
                } else {
                    u.models.insert(agent.clone(), model);
                }
                u.agent = Some(agent);
                u.save(&sd);
            }
            AgentRequest::Consent => {
                let sd = self.state_dir();
                let mut u = UserSettings::load(&sd);
                let agent = self.status().agent.ok_or("no agent is installed")?;
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs());
                u.consent.insert(agent, now);
                u.save(&sd);
            }
            AgentRequest::OpenTerminal { text } => self.open_terminal(&text)?,
            AgentRequest::ApplyDraft => {
                if self.lock().running.is_some() {
                    return Err("wait for the agent to finish".into());
                }
                let dir = self.project_dir().ok_or("no project is open")?;
                let queries = match self.lock().draft.take() {
                    Some((d, q)) if d == dir => q,
                    _ => return Err("there's no draft to apply".into()),
                };
                let state = self
                    .app()
                    .handle(charted_core::Request::ApplyQueries { queries })
                    .map_err(|e| e.message)?;
                (self.emit)(Push::State(Box::new(state)));
            }
            AgentRequest::DiscardDraft => self.lock().draft = None,
            AgentRequest::SetInstructions { text } => {
                let dir = self.project_dir().ok_or("open a project first")?;
                crate::settings::set_instructions(&dir, &text)?;
            }
        }
        Ok(self.push_status())
    }

    fn new_turn(&self, label: &str, data: bool, turn: &str) -> (String, Arc<TurnCtx>) {
        let t = token();
        let ctx = Arc::new(TurnCtx {
            host: self.me.clone(),
            turn: turn.into(),
            group: turn.into(),
            label: label.into(),
            data,
        });
        self.lock().tokens.insert(t.clone(), ctx.clone());
        (t, ctx)
    }

    /// The page and visuals the user is looking at, in words for the agent.
    fn context(&self, page: Option<&str>, visuals: &[String]) -> Option<(String, String)> {
        let app = self.app();
        let m = &app.session()?.project.model;
        let p = m.report.page(page?)?;
        let shown: Vec<String> = visuals
            .iter()
            .filter_map(|id| p.visual(id))
            .map(|v| {
                format!(
                    "\"{}\" ({}, id {})",
                    charted_core::visualq::visual_title(m, v),
                    v.visual_type,
                    v.id
                )
            })
            .collect();
        let mut agent = format!(
            "The user is on the page \"{}\" (id {}).",
            p.settings.name, p.id
        );
        if !shown.is_empty() {
            agent.push_str(&format!(" Selected: {}.", shown.join(", ")));
        }
        let user = if shown.is_empty() {
            p.settings.name.clone()
        } else {
            format!("{} · {}", p.settings.name, shown.len())
        };
        Some((agent, user))
    }

    fn send(&self, text: &str, page: Option<&str>, visuals: &[String]) -> Result<(), String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("type a request".into());
        }
        let dir = self.project_dir().ok_or("open a project first")?;
        if self.lock().running.is_some() {
            return Err("the agent is still working on the last request".into());
        }
        let status = self.status();
        let agent = status.agent.clone().ok_or("no agent is installed")?;
        if !status.consented {
            return Err("consent".into());
        }
        let info = status
            .agents
            .iter()
            .find(|a| a.name == agent)
            .ok_or("that agent isn't installed")?;
        if !info.headless {
            return Err(format!(
                "{} runs in a terminal; use Open in terminal",
                info.label
            ));
        }
        let first = text.lines().next().unwrap_or(text);
        let short: String = first.chars().take(48).collect();
        let label = format!(
            "Agent: {}{}",
            short,
            if first.chars().count() > 48 {
                "…"
            } else {
                ""
            }
        );
        let turn_id = uuid::Uuid::new_v4().to_string();
        let ctx = self.context(page, visuals);
        let record = TurnRecord {
            id: turn_id.clone(),
            group: turn_id.clone(),
            request: text.into(),
            context: ctx.as_ref().map(|c| c.1.clone()),
            agent: agent.clone(),
            model: (!status.model.is_empty()).then(|| status.model.clone()),
            status: TurnStatus::Running,
            cost_usd: None,
            items: vec![],
        };
        let session = self.with_chat(|c| {
            c.turns.push(record.clone());
            c.sessions.get(&agent).cloned()
        });
        let (tok, _) = self.new_turn(&label, status.data, &turn_id);
        let mut prompt = match &ctx {
            Some((c, _)) => format!("{text}\n\n({c})"),
            None => text.to_string(),
        };
        // Read every request, so edits apply to a resumed conversation too.
        let instructions = crate::settings::instructions(&dir);
        if !instructions.trim().is_empty() {
            prompt.push_str(&format!(
                "\n\nThe project's instructions (agent.md, from the user; follow them):\n{}",
                instructions.trim()
            ));
        }
        let mut cmd = self.command(&agent, &status.model, session.flatten(), &tok, &dir)?;
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("couldn't start {agent}: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(prompt.as_bytes());
        }
        let stdout = child.stdout.take().expect("piped");
        let stderr = child.stderr.take().expect("piped");
        let child = Arc::new(Mutex::new(child));
        self.lock().running = Some(Running {
            turn: turn_id.clone(),
            child: child.clone(),
            stopped: false,
        });
        (self.emit)(Push::Agent(AgentEvent::Turn { turn: record }));
        let me = self.me.clone();
        let err_tail = Arc::new(Mutex::new(String::new()));
        {
            let tail = err_tail.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    let mut t = tail.lock().unwrap_or_else(|p| p.into_inner());
                    t.push_str(&line);
                    t.push('\n');
                    if t.len() > 4000 {
                        let cut = t.len() - 2000;
                        let at = t.ceil_char_boundary(cut);
                        *t = t[at..].to_string();
                    }
                }
            });
        }
        std::thread::spawn(move || {
            let parse = if agent == "codex" {
                stream::codex
            } else {
                stream::claude
            };
            let mut finished: Option<(bool, Option<String>)> = None;
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let Some(h) = me.upgrade() else { return };
                for ev in parse(&line) {
                    match ev {
                        Event::Session(s) => {
                            h.with_chat(|c| c.sessions.insert(agent.clone(), s));
                        }
                        Event::Text(t) => h.record(&turn_id, Item::Text { text: t }),
                        Event::Cost(c) => h.update_turn(&turn_id, |t| t.cost_usd = Some(c)),
                        Event::Forbidden(tool) => {
                            h.record(
                                &turn_id,
                                Item::Error {
                                    message: format!("{agent} offered its own tool {tool}, which could read files; charted stopped it."),
                                },
                            );
                            h.kill(&child);
                        }
                        Event::Finished { ok, message } => finished = Some((ok, message)),
                    }
                }
            }
            let code = child.lock().unwrap_or_else(|p| p.into_inner()).wait().ok();
            let Some(h) = me.upgrade() else { return };
            let stopped = {
                let mut inner = h.lock();
                inner.tokens.retain(|_, c| c.turn != turn_id);
                let stopped = inner.running.as_ref().is_some_and(|r| r.stopped);
                inner.running = None;
                stopped
            };
            let status = match (&finished, stopped) {
                (_, true) => TurnStatus::Stopped,
                (Some((true, _)), _) => TurnStatus::Done,
                _ => TurnStatus::Failed,
            };
            let message = match (&finished, status) {
                (Some((false, Some(m))), TurnStatus::Failed) => Some(m.clone()),
                (None, TurnStatus::Failed) => {
                    let tail = err_tail
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .trim()
                        .to_string();
                    Some(match tail.lines().last() {
                        Some(l) => format!("{agent} stopped: {l}"),
                        None => format!(
                            "{agent} stopped ({})",
                            code.map_or("no exit code".into(), |c| c.to_string())
                        ),
                    })
                }
                _ => None,
            };
            h.update_turn(&turn_id, |t| {
                t.status = status;
                if let Some(m) = message {
                    t.items.push(Item::Error { message: m });
                }
            });
            h.push_status();
        });
        Ok(())
    }

    /// The command that runs `agent` for one request.
    fn command(
        &self,
        agent: &str,
        model: &str,
        session: Option<String>,
        tok: &str,
        project: &Path,
    ) -> Result<Command, String> {
        let work = crate::transcript::dir(project).join("work");
        std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
        let exe = std::env::var_os("CHARTED_AGENT_MCP")
            .map(PathBuf::from)
            .or_else(|| std::env::current_exe().ok())
            .ok_or("can't find charted's own program")?;
        let mcp_args = vec![
            "mcp".to_string(),
            "--socket".to_string(),
            self.socket.display().to_string(),
        ];
        let prompt = include_str!("prompt.md");
        let fake = std::env::var("CHARTED_AGENT_CMD")
            .ok()
            .filter(|_| agent == "claude");
        let mut cmd = match (agent, fake) {
            ("claude", fake) => {
                let config = json!({ "mcpServers": { "charted": { "command": exe, "args": mcp_args, "env": { "CHARTED_AGENT_TOKEN": tok } } } });
                let mut c = match fake {
                    Some(f) => {
                        let mut parts = f.split_whitespace();
                        let mut c = Command::new(parts.next().ok_or("CHARTED_AGENT_CMD is empty")?);
                        c.args(parts);
                        c
                    }
                    None => Command::new("claude"),
                };
                c.args(["-p", "--output-format", "stream-json", "--verbose"])
                    .arg("--mcp-config")
                    .arg(config.to_string())
                    .args([
                        "--strict-mcp-config",
                        "--tools",
                        "",
                        "--allowedTools",
                        "mcp__charted__*",
                    ])
                    .args([
                        "--permission-mode",
                        "dontAsk",
                        "--setting-sources",
                        "project",
                    ])
                    .arg("--max-turns")
                    .arg(MAX_TURNS.to_string())
                    .arg("--append-system-prompt")
                    .arg(prompt);
                if !model.is_empty() {
                    c.args(["--model", model]);
                }
                match session {
                    Some(s) => c.args(["--resume", &s]),
                    None => c.args(["--session-id", &uuid::Uuid::new_v4().to_string()]),
                };
                c
            }
            ("codex", _) => {
                let mut c = Command::new("codex");
                c.arg("exec");
                if let Some(s) = &session {
                    c.args(["resume", s]);
                }
                c.args(["--json", "--skip-git-repo-check", "-s", "read-only"]);
                for f in [
                    "shell_tool",
                    "unified_exec",
                    "view_image",
                    "apps",
                    "plugins",
                    "browser_use",
                    "browser_use_external",
                    "computer_use",
                    "image_generation",
                    "multi_agent",
                    "in_app_browser",
                ] {
                    c.args(["--disable", f]);
                }
                let toml = |v: &str| serde_json::to_string(v).expect("a string");
                c.arg("-c").arg("approval_policy=\"never\"");
                c.arg("-c").arg(format!(
                    "mcp_servers.charted.command={}",
                    toml(&exe.display().to_string())
                ));
                c.arg("-c").arg(format!(
                    "mcp_servers.charted.args=[{}]",
                    mcp_args
                        .iter()
                        .map(|a| toml(a))
                        .collect::<Vec<_>>()
                        .join(",")
                ));
                c.arg("-c").arg(format!(
                    "mcp_servers.charted.env={{CHARTED_AGENT_TOKEN={}}}",
                    toml(tok)
                ));
                c.arg("-c")
                    .arg(format!("developer_instructions={}", toml(prompt)));
                if !model.is_empty() {
                    c.args(["-m", model]);
                }
                c.arg("-");
                c
            }
            (a, _) => return Err(format!("{a} can't run inside charted")),
        };
        cmd.current_dir(&work);
        // A charted started from an agent's terminal mustn't pass that agent's session on.
        for (k, _) in std::env::vars_os() {
            let k = k.to_string_lossy().to_string();
            if k == "CLAUDECODE"
                || k == "CLAUDE_PID"
                || k == "CLAUDE_EFFORT"
                || k.starts_with("CLAUDE_CODE_")
            {
                cmd.env_remove(&k);
            }
        }
        // The account Omarchy has active for the agent.
        let home_env = if agent == "codex" {
            "CODEX_HOME"
        } else {
            "CLAUDE_CONFIG_DIR"
        };
        if std::env::var_os(home_env).is_none()
            && let Ok(out) = Command::new("omarchy-agent-account-home")
                .arg(agent)
                .output()
        {
            let home = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !home.is_empty() {
                cmd.env(home_env, home);
            }
        }
        // Its own process group, so Stop reaches everything it started.
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
        Ok(cmd)
    }

    fn kill(&self, child: &Arc<Mutex<Child>>) {
        let pid = child.lock().unwrap_or_else(|p| p.into_inner()).id() as i32;
        // SAFETY: signals the agent's own process group.
        unsafe { libc::kill(-pid, libc::SIGTERM) };
        let child = child.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(3));
            if let Ok(None) = child.lock().unwrap_or_else(|p| p.into_inner()).try_wait() {
                // SAFETY: as above.
                unsafe { libc::kill(-pid, libc::SIGKILL) };
            }
        });
    }

    /// Stops the running request: its tools stop answering, questions are answered no,
    /// and the agent is ended.
    pub fn stop(&self) {
        let child = {
            let mut inner = self.lock();
            let Some(r) = inner.running.as_mut() else {
                return;
            };
            r.stopped = true;
            let turn = r.turn.clone();
            let child = r.child.clone();
            inner.tokens.retain(|_, c| c.turn != turn);
            for (_, tx) in inner.pending.drain() {
                let _ = tx.send(false);
            }
            child
        };
        self.kill(&child);
    }

    fn open_terminal(&self, text: &str) -> Result<(), String> {
        let status = self.status();
        let agent = status.agent.clone().ok_or("no agent is installed")?;
        let dir = self.project_dir().ok_or("open a project first")?;
        let turn = uuid::Uuid::new_v4().to_string();
        let (tok, _) = self.new_turn("Agent (terminal)", status.data, &turn);
        self.with_chat(|c| {
            c.turns.push(TurnRecord {
                id: turn.clone(),
                group: turn.clone(),
                request: text.into(),
                context: None,
                agent: agent.clone(),
                model: None,
                status: TurnStatus::Running,
                cost_usd: None,
                items: vec![],
            })
        });
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let work = crate::transcript::dir(&dir).join("terminal");
        std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
        let server = json!({ "command": exe, "args": ["mcp", "--socket", self.socket], "env": { "CHARTED_AGENT_TOKEN": tok } });
        // Project-local MCP settings, in the forms the agents read.
        let _ = std::fs::write(
            work.join(".mcp.json"),
            json!({ "mcpServers": { "charted": server } }).to_string(),
        );
        let _ = std::fs::create_dir_all(work.join(".cursor"));
        let _ = std::fs::write(
            work.join(".cursor/mcp.json"),
            json!({ "mcpServers": { "charted": server } }).to_string(),
        );
        let _ = std::fs::write(
            work.join("opencode.json"),
            json!({ "mcp": { "charted": { "type": "local", "command": [exe, "mcp", "--socket", self.socket], "environment": { "CHARTED_AGENT_TOKEN": tok } } } }).to_string(),
        );
        let _ = std::fs::create_dir_all(work.join(".gemini"));
        let _ = std::fs::write(
            work.join(".gemini/settings.json"),
            json!({ "mcpServers": { "charted": server } }).to_string(),
        );
        let _ = std::fs::write(work.join("AGENTS.md"), include_str!("prompt.md"));
        let _ = std::fs::write(work.join("GEMINI.md"), include_str!("prompt.md"));
        let prompt = if text.trim().is_empty() {
            "Use charted's tools to help me with my report.".to_string()
        } else {
            text.to_string()
        };
        let cmd = terminal_command(&agent, &prompt)
            .ok_or_else(|| format!("{agent} can't be opened from charted"))?;
        // The agent picked in the pane, in Omarchy's terminal for agents (not Omarchy's
        // default agent, which omarchy-agent-prompt would start).
        let mut launch = if on_path("omarchy-launch-tui") {
            let mut c = Command::new("omarchy-launch-tui");
            c.arg("--app-id=org.omarchy.agent");
            c
        } else {
            let mut c = Command::new("xdg-terminal-exec");
            c.arg("-e");
            c
        };
        launch
            .args(&cmd)
            .current_dir(&work)
            .spawn()
            .map_err(|e| format!("couldn't open a terminal: {e}"))?;
        Ok(())
    }

    /// Serves one connection of `charted mcp`: `{"hello": token}`, then tool calls.
    fn serve(self: Arc<Self>, conn: UnixStream) {
        let Ok(read) = conn.try_clone() else { return };
        let mut out = conn;
        let mut lines = BufReader::new(read).lines();
        let Some(Ok(hello)) = lines.next() else {
            return;
        };
        let tok = serde_json::from_str::<Value>(&hello)
            .ok()
            .and_then(|v| v["hello"].as_str().map(str::to_string));
        let ctx = tok.and_then(|t| self.lock().tokens.get(&t).cloned());
        let Some(ctx) = ctx else {
            let _ = writeln!(
                out,
                "{}",
                json!({ "error": "charted didn't recognise this agent; start it from charted's Agent pane" })
            );
            return;
        };
        let _ = writeln!(out, "{}", json!({ "ok": true }));
        for line in lines.map_while(Result::ok) {
            let Ok(req) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let id = req["id"].clone();
            let live = self.lock().tokens.values().any(|c| Arc::ptr_eq(c, &ctx));
            let result = match req["method"].as_str() {
                _ if !live => {
                    json!({ "content": [{ "type": "text", "text": "The user stopped this request; stop working." }], "isError": true })
                }
                Some("tools/list") => {
                    let list: Vec<Value> = tools::list()
                        .into_iter()
                        .filter(|t| ctx.data || !tools::DATA_TOOLS.contains(&t.name))
                        .map(|t| json!({ "name": t.name, "description": t.description, "inputSchema": t.input_schema }))
                        .collect();
                    json!({ "tools": list })
                }
                Some("tools/call") => {
                    let name = req["params"]["name"].as_str().unwrap_or("");
                    let args = req["params"]["arguments"].clone();
                    let args = if args.is_null() { json!({}) } else { args };
                    let o = tools::call(&self.app, &*ctx, name, &args);
                    if let Some(step) = o.step {
                        self.record(
                            &ctx.turn,
                            Item::Step {
                                step: Step {
                                    tool: name.into(),
                                    ..step
                                },
                            },
                        );
                    }
                    json!({ "content": [{ "type": "text", "text": o.text }], "isError": o.is_error })
                }
                _ => {
                    json!({ "content": [{ "type": "text", "text": "unknown method" }], "isError": true })
                }
            };
            if writeln!(out, "{}", json!({ "id": id, "result": result })).is_err() {
                break;
            }
        }
    }

    /// A token for driving the tools by hand (tests, and a terminal agent pointed at a
    /// running charted while developing).
    pub fn test_token(&self, label: &str, data: bool) -> String {
        let turn = uuid::Uuid::new_v4().to_string();
        self.new_turn(label, data, &turn).0
    }
}
