//! Command-line interface. Every command needs no display, prints human-readable output
//! by default or JSON with `--json`, writes errors to stderr and exits 0 or 1.

use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
#[command(
    name = "charted",
    version,
    about = "Load data, model it, and build interactive reports. Locally."
)]
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
    /// Run SQL against a project's loaded data, or show a report visual's data.
    Query {
        project: PathBuf,
        /// DuckDB SQL (omit with --visual).
        #[arg(required_unless_present = "visual")]
        sql: Option<String>,
        /// A report visual, by id or title: prints its rows by field name.
        #[arg(long, conflicts_with = "sql")]
        visual: Option<String>,
        /// With --visual: print the generated SQL instead of running it.
        #[arg(long, requires = "visual")]
        show_sql: bool,
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
    /// Export a project's model: a Power BI project (PBIP with TMDL) or Apache Ossie.
    Export {
        #[command(subcommand)]
        format: ExportFormat,
    },
    /// Import an Apache Ossie document into a project, replacing its model (the report
    /// is kept). Prints what changes; --dry-run only prints.
    Import {
        project: PathBuf,
        file: PathBuf,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        json: bool,
    },
    /// Import a Power BI project (.pbip, its folder, or a model.bim) as a new charted
    /// project, and report what didn't come across.
    ImportPbip {
        input: PathBuf,
        /// The folder to create the project in (default: Documents/charted).
        #[arg(long)]
        into: Option<PathBuf>,
        #[arg(long)]
        json: bool,
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
pub enum ExportFormat {
    /// A folder Power BI Desktop opens: <dir>/<Name>.pbip and its TMDL model.
    Pbip { project: PathBuf, dir: PathBuf },
    /// One Apache Ossie YAML document.
    Ossie { project: PathBuf, file: PathBuf },
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

fn import_pbip(input: &Path, into: Option<PathBuf>, json: bool) -> i32 {
    let parent = into.unwrap_or_else(charted_core::api::projects_dir);
    let (dir, report) = match charted_core::pbimport::import(input, &parent) {
        Ok(r) => r,
        Err(e) => return fail(e),
    };
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "project": dir, "report": report }))
                .unwrap()
        );
        return 0;
    }
    println!("Imported {} into {}", input.display(), dir.display());
    println!(
        "  {} tables ({} with data), {} measures, {} relationships, {} pages, {} visuals",
        report.tables,
        report.loaded,
        report.measures,
        report.relationships,
        report.pages,
        report.visuals
    );
    for n in &report.notes {
        println!("  note: {n}");
    }
    if !report.skipped.is_empty() {
        println!("Not imported, or only partly ({}):", report.skipped.len());
        for s in &report.skipped {
            println!("  {} {}: {}", s.kind, s.name, s.reason);
        }
    }
    0
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
        Command::Query {
            project,
            sql,
            visual,
            show_sql,
            json,
            csv,
            limit,
        } => match visual {
            Some(v) => query_visual(&project, &v, show_sql, json, csv, limit),
            None => query(
                &project,
                sql.as_deref().unwrap_or_default(),
                json,
                csv,
                limit,
            ),
        },
        Command::Sample {
            action: SampleAction::Create { dir, scale, json },
        } => {
            let Some(scale) = charted_samplegen::Scale::parse(&scale) else {
                return fail(format!("unknown scale \"{scale}\"; use demo, 1m or 10m"));
            };
            match charted_core::sample::create(&dir, scale) {
                Ok(p) => {
                    if json {
                        println!(
                            "{}",
                            serde_json::json!({ "ok": true, "path": p.manifest_path, "tables": p.model.tables.len() })
                        );
                    } else {
                        println!("Created {} in {}", p.manifest.name, p.dir.display());
                    }
                    0
                }
                Err(e) => fail(e),
            }
        }
        #[cfg(feature = "dev-server")]
        Command::Serve {
            port,
            token,
            state_dir,
        } => match devserver::serve(port, &token, state_dir) {
            Ok(()) => 0,
            Err(e) => fail(e),
        },
        Command::Export { format } => export(format),
        Command::Import {
            project,
            file,
            dry_run,
            json,
        } => import(&project, &file, dry_run, json),
        Command::ImportPbip { input, into, json } => import_pbip(&input, into, json),
        Command::Skill {
            action: SkillAction::Show,
        } => {
            print!("{SKILL}");
            0
        }
        Command::Skill {
            action: SkillAction::Install,
        } => match install_skill() {
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

fn query_visual(
    project: &Path,
    which: &str,
    show_sql: bool,
    json: bool,
    csv: bool,
    limit: u64,
) -> i32 {
    use charted_core::visualq;
    let session = match charted_core::Session::open(project) {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    let model = &session.project.model;
    let found: Vec<&charted_model::Visual> = model
        .report
        .pages
        .iter()
        .flat_map(|p| &p.visuals)
        .filter(|v| v.id == which || visualq::visual_title(model, v).eq_ignore_ascii_case(which))
        .collect();
    let v = match found.as_slice() {
        [v] => *v,
        [] => {
            let titles: Vec<String> = model
                .report
                .pages
                .iter()
                .flat_map(|p| {
                    p.visuals.iter().map(move |v| {
                        format!(
                            "  {} ({}, page {})",
                            visualq::visual_title(model, v),
                            v.id,
                            p.settings.name
                        )
                    })
                })
                .collect();
            return fail(format!(
                "no visual is called \"{which}\". Visuals:\n{}",
                titles.join("\n")
            ));
        }
        _ => {
            return fail(format!(
                "more than one visual is called \"{which}\"; use its id"
            ));
        }
    };
    let sql_text = match visualq::report_visual_sql(
        model,
        session.schema(),
        &v.id,
        &visualq::VisualSqlOptions {
            captions: true,
            ..Default::default()
        },
    ) {
        Ok(x) => x,
        Err(e) => return fail(e),
    };
    if show_sql {
        println!("{sql_text}");
        return 0;
    }
    let db = session.database();
    if csv {
        return match db.csv(&sql_text) {
            Ok(text) => {
                print!("{text}");
                0
            }
            Err(e) => fail(e),
        };
    }
    match db.json_rows(&sql_text, (limit > 0).then_some(limit)) {
        Ok(rows) => {
            if json {
                println!("[{}]", rows.join(","));
            } else {
                println!("{}", visualq::visual_title(model, v));
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
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| std::io::Error::other("HOME is not set"))?;
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

fn export(format: ExportFormat) -> i32 {
    let (project, out) = match &format {
        ExportFormat::Pbip { project, dir } => (project, dir),
        ExportFormat::Ossie { project, file } => (project, file),
    };
    let p = match charted_core::project::Project::open(project) {
        Ok(p) => p,
        Err(e) => return fail(e),
    };
    let done = match format {
        ExportFormat::Pbip { .. } => charted_core::interop::export_pbip(&p, out),
        ExportFormat::Ossie { .. } => {
            charted_core::interop::export_ossie(&p.model, out).map(|()| out.clone())
        }
    };
    match done {
        Ok(path) => {
            println!("Wrote {}", path.display());
            0
        }
        Err(e) => fail(e),
    }
}

fn import(project: &Path, file: &Path, dry_run: bool, json: bool) -> i32 {
    let p = match charted_core::project::Project::open(project) {
        Ok(p) => p,
        Err(e) => return fail(e),
    };
    let diff = match charted_core::interop::import_diff(&p.model, file) {
        Ok((_, d)) => d,
        Err(e) => return fail(e),
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&diff).unwrap());
    } else {
        for w in &diff.warnings {
            eprintln!("warning: {w}");
        }
        if diff.differences.is_empty() {
            println!("0 differences");
        } else {
            println!("{} difference(s):", diff.differences.len());
            for d in &diff.differences {
                println!("  {d}");
            }
        }
    }
    if dry_run || diff.differences.is_empty() {
        return 0;
    }
    let mut session = match charted_core::Session::open(project) {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    match session.import_ossie(file).and_then(|()| session.save()) {
        Ok(()) => {
            if !json {
                println!("Imported into {}", project.display());
            }
            0
        }
        Err(e) => fail(e),
    }
}
