//! Command-line interface. Every command needs no display, prints human-readable output
//! by default or JSON with `--json`, writes errors to stderr and exits 0 or 1.

use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
#[command(name = "charted", version, about = "Load data, model it, and build interactive reports. Locally.")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Open the app, optionally with a project folder or .charted file.
    Open {
        path: Option<PathBuf>,
        /// Save a screenshot of the window to FILE once the project has loaded, then exit.
        #[arg(long, value_name = "FILE")]
        screenshot: Option<PathBuf>,
    },
    /// Validate a project: model schema, queries and source files.
    Check {
        project: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Run SQL against a project's loaded data.
    Query {
        project: PathBuf,
        sql: String,
        #[arg(long)]
        json: bool,
        #[arg(long, conflicts_with = "json")]
        csv: bool,
        /// Maximum rows to print (default 100; 0 for all).
        #[arg(long, default_value_t = 100)]
        limit: u64,
    },
    /// Create the Harbor & Pine sample project.
    Sample {
        #[command(subcommand)]
        action: SampleAction,
    },
    /// Serve the request API over a local WebSocket for browser-based development and
    /// end-to-end tests.
    #[cfg(feature = "dev-server")]
    Serve {
        #[arg(long, default_value_t = 7420)]
        port: u16,
        /// Required token; clients connect to ws://127.0.0.1:PORT/?token=TOKEN.
        #[arg(long, env = "CHARTED_DEV_TOKEN", default_value = "")]
        token: String,
        /// Directory for recent-projects state (defaults to a temporary directory).
        #[arg(long)]
        state_dir: Option<PathBuf>,
    },
    /// Install the bundled agent skill so coding agents can drive charted.
    Skill {
        #[command(subcommand)]
        action: SkillAction,
    },
}

#[derive(Debug, Subcommand)]
pub enum SampleAction {
    Create {
        dir: PathBuf,
        /// demo (~250k sales lines), 1m or 10m.
        #[arg(long, default_value = "demo")]
        scale: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum SkillAction {
    Install,
    /// Print the skill.
    Show,
}

#[cfg(feature = "dev-server")]
mod devserver;

pub const SKILL: &str = include_str!("../../../skill/SKILL.md");

fn fail(msg: impl std::fmt::Display) -> i32 {
    eprintln!("charted: {msg}");
    1
}

/// Runs a display-free command and returns the process exit code.
pub fn run(command: Command) -> i32 {
    match command {
        Command::Open { .. } => fail("`open` needs the desktop app"),
        Command::Check { project, json } => {
            let report = charted_core::check::check(&project);
            if json {
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                match &report.project {
                    Some(name) if report.ok => println!("{name}: OK"),
                    Some(name) => println!("{name}: {} problem(s)", report.errors.len()),
                    None => {}
                }
                for e in &report.errors {
                    eprintln!("error: {e}");
                }
                for w in &report.warnings {
                    eprintln!("warning: {w}");
                }
            }
            if report.ok { 0 } else { 1 }
        }
        Command::Query { project, sql, json, csv, limit } => query(&project, &sql, json, csv, limit),
        Command::Sample { action: SampleAction::Create { dir, scale, json } } => {
            let Some(scale) = charted_samplegen::Scale::parse(&scale) else {
                return fail(format!("unknown scale \"{scale}\"; use demo, 1m or 10m"));
            };
            match charted_core::sample::create(&dir, scale) {
                Ok(p) => {
                    if json {
                        println!("{}", serde_json::json!({ "ok": true, "path": p.manifest_path, "tables": p.model.tables.len() }));
                    } else {
                        println!("Created {} in {}", p.manifest.name, p.dir.display());
                    }
                    0
                }
                Err(e) => fail(e),
            }
        }
        #[cfg(feature = "dev-server")]
        Command::Serve { port, token, state_dir } => match devserver::serve(port, &token, state_dir) {
            Ok(()) => 0,
            Err(e) => fail(e),
        },
        Command::Skill { action: SkillAction::Show } => {
            print!("{SKILL}");
            0
        }
        Command::Skill { action: SkillAction::Install } => match install_skill() {
            Ok(path) => {
                println!("Installed the charted skill to {}", path.display());
                0
            }
            Err(e) => fail(e),
        },
    }
}

fn query(project: &Path, sql: &str, json: bool, csv: bool, limit: u64) -> i32 {
    let session = match charted_core::Session::open(project) {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    let db = session.database();
    let limit = (limit > 0).then_some(limit);
    if csv {
        let sql = match limit {
            Some(n) => format!("SELECT * FROM ({sql}) LIMIT {n}"),
            None => sql.to_string(),
        };
        return match db.csv(&sql) {
            Ok(text) => {
                print!("{text}");
                0
            }
            Err(e) => fail(e),
        };
    }
    match db.json_rows(sql, limit) {
        Ok(rows) => {
            if json {
                println!("[{}]", rows.join(","));
            } else {
                for r in &rows {
                    println!("{r}");
                }
            }
            0
        }
        Err(e) => fail(e),
    }
}

/// Copies the skill to ~/.agents/skills/charted and links it into ~/.claude/skills.
fn install_skill() -> std::io::Result<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| std::io::Error::other("HOME is not set"))?;
    let dir = home.join(".agents/skills/charted");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("SKILL.md"), SKILL)?;
    let claude = home.join(".claude/skills");
    std::fs::create_dir_all(&claude)?;
    let link = claude.join("charted");
    if link.symlink_metadata().is_err() {
        std::os::unix::fs::symlink(&dir, &link)?;
    }
    Ok(dir)
}
