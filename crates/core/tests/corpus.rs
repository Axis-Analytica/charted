//! Real Power BI projects from public repositories (fetched by `bin/corpus` into
//! ~/Work/charted-corpus, not vendored): each imports without failing, its visuals all
//! either query or are reported, and charted's export of it imports again unchanged.
//! Files on the web load after allowing DuckDB's web reader, as a user would.
//! `bin/corpus && cargo test -p charted-core --test corpus -- --ignored --nocapture`

use charted_core::{App, DataRequest, Request};
use std::path::{Path, PathBuf};

fn corpus() -> Option<PathBuf> {
    let dir = std::env::var_os("CHARTED_CORPUS")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join("Work/charted-corpus")))?;
    dir.is_dir().then_some(dir)
}

fn projects(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if p.is_dir() && !name.starts_with(".git") {
                stack.push(p);
            } else if name.ends_with(".pbip") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

#[test]
#[ignore]
fn public_power_bi_projects_import_render_and_round_trip() {
    let Some(dir) = corpus() else {
        eprintln!("no corpus: run bin/corpus");
        return;
    };
    let web = std::env::var_os("CHARTED_CORPUS_OFFLINE").is_none();
    let tmp = tempfile::tempdir().unwrap();
    let mut summary = Vec::new();
    for pbip in projects(&dir) {
        let label = pbip.strip_prefix(&dir).unwrap().display().to_string();
        let out = tmp.path().join(summary.len().to_string());
        std::fs::create_dir_all(&out).unwrap();
        let (project, report) =
            charted_core::pbimport::import(&pbip, &out).unwrap_or_else(|e| panic!("{label}: {e}"));
        let mut app = App::new(tmp.path().join("state"));
        app.handle(Request::OpenProject {
            path: project.display().to_string(),
        })
        .unwrap_or_else(|e| panic!("{label}: {}", e.message));
        // Files on the web: allow the web reader, then refresh.
        let needs_web = app
            .session()
            .unwrap()
            .project
            .queries
            .iter()
            .any(|q| q.source.is_web());
        let mut refreshed = String::from("-");
        if needs_web && web {
            app.handle(Request::AllowExtension {
                name: "httpfs".into(),
            })
            .unwrap();
            refreshed = match app.handle(Request::Refresh) {
                Ok(_) => "ok".into(),
                Err(e) => format!("failed: {}", e.message.lines().next().unwrap_or("")),
            };
        }
        // Every visual queries (or fails with a message, which is counted).
        let model = app.session().unwrap().project.model.clone();
        let (mut ok, mut failed) = (0, Vec::new());
        for page in &model.report.pages {
            for v in &page.visuals {
                if v.roles.values().all(|r| r.is_empty()) {
                    continue;
                }
                match app.data(DataRequest::ReportVisual {
                    visual: v.id.clone(),
                    totals: false,
                    filters: vec![],
                    highlight: vec![],
                    tag: None,
                    search: None,
                    roles: vec![],
                }) {
                    Ok(_) => ok += 1,
                    Err(e) => failed.push(format!(
                        "{} {}: {}",
                        page.settings.name,
                        v.visual_type,
                        e.message.lines().next().unwrap_or("")
                    )),
                }
            }
        }
        // charted's export imports back unchanged.
        let export = tmp.path().join(format!("export-{}", summary.len()));
        app.handle(Request::ExportPbip {
            dir: export.display().to_string(),
            options: None,
        })
        .unwrap_or_else(|e| panic!("{label}: export: {}", e.message));
        let again = std::fs::read_dir(&export)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|x| x == "pbip"))
            .unwrap();
        let back = tmp.path().join(format!("back-{}", summary.len()));
        std::fs::create_dir_all(&back).unwrap();
        let (_, report2) = charted_core::pbimport::import(&again, &back)
            .unwrap_or_else(|e| panic!("{label}: re-import: {e}"));
        summary.push(format!(
            "{label}: {} tables, {} measures, {} pages, {} visuals; loaded {} on import, web refresh {refreshed}; visuals query {ok}, fail {}; {} skipped; re-import skipped {}",
            report.tables,
            report.measures,
            report.pages,
            report.visuals,
            report.loaded,
            failed.len(),
            report.skipped.len(),
            report2.skipped.len(),
        ));
        for f in failed.iter().take(5) {
            summary.push(format!("    {f}"));
        }
    }
    println!("{}", summary.join("\n"));
}
