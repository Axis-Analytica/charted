//! Report files, one per page and per visual so edits make small git diffs:
//!
//! ```text
//! report/report.json                        page order and report-level filters
//! report/pages/<page>/page.json             name, size, page filters
//! report/pages/<page>/visuals/<visual>.json one visual
//! ```

use charted_model::{Page, PageSettings, Report, ReportFilter, Visual};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::atomic;
use crate::project::ProjectError;

pub const REPORT_DIR: &str = "report";
const REPORT_FORMAT: u32 = 1;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReportFile {
    format: u32,
    /// Page ids in display order.
    pages: Vec<String>,
    #[serde(default)]
    filters: Vec<ReportFilter>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    diagram: std::collections::BTreeMap<String, charted_model::DiagramPosition>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageFile {
    id: String,
    #[serde(flatten)]
    settings: PageSettings,
    /// Visual ids in stacking/tab order.
    #[serde(default)]
    visuals: Vec<String>,
}

/// Ids become folder and file names, so only plain ones are accepted.
fn safe(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn invalid(path: &Path, message: impl std::fmt::Display) -> ProjectError {
    ProjectError::Invalid {
        path: path.to_path_buf(),
        message: message.to_string(),
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, ProjectError> {
    let text = std::fs::read_to_string(path).map_err(|source| ProjectError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_str(&text).map_err(|e| invalid(path, e))
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), ProjectError> {
    let mut json = serde_json::to_string_pretty(value).expect("report serialises");
    json.push('\n');
    // Unchanged files are left alone (keeps timestamps and avoids needless writes).
    if std::fs::read_to_string(path).is_ok_and(|old| old == json) {
        return Ok(());
    }
    atomic::write(path, json.as_bytes()).map_err(|source| ProjectError::Io {
        path: path.to_path_buf(),
        source,
    })
}

pub fn read(project_dir: &Path) -> Result<Report, ProjectError> {
    let dir = project_dir.join(REPORT_DIR);
    let index = dir.join("report.json");
    if !index.exists() {
        return Ok(Report::default());
    }
    let file: ReportFile = read_json(&index)?;
    let mut report = Report {
        pages: Vec::new(),
        filters: file.filters,
        diagram: file.diagram,
    };
    for id in file.pages.iter().filter(|id| safe(id)) {
        let pdir = dir.join("pages").join(id);
        let pf: PageFile = read_json(&pdir.join("page.json"))?;
        let mut visuals = Vec::new();
        for vid in pf.visuals.iter().filter(|v| safe(v)) {
            let vpath = pdir.join("visuals").join(format!("{vid}.json"));
            if vpath.exists() {
                visuals.push(read_json::<Visual>(&vpath)?);
            }
        }
        report.pages.push(Page {
            id: id.clone(),
            settings: pf.settings,
            visuals,
        });
    }
    Ok(report)
}

pub fn write(project_dir: &Path, report: &Report) -> Result<(), ProjectError> {
    let dir = project_dir.join(REPORT_DIR);
    let pages_dir = dir.join("pages");
    let mut keep_pages: Vec<PathBuf> = Vec::new();
    for p in &report.pages {
        if !safe(&p.id) {
            return Err(invalid(
                &pages_dir,
                format!("page id \"{}\" is not a plain identifier", p.id),
            ));
        }
        let pdir = pages_dir.join(&p.id);
        let vdir = pdir.join("visuals");
        let mut keep_visuals = Vec::new();
        for v in &p.visuals {
            if !safe(&v.id) {
                return Err(invalid(
                    &vdir,
                    format!("visual id \"{}\" is not a plain identifier", v.id),
                ));
            }
            let path = vdir.join(format!("{}.json", v.id));
            write_json(&path, v)?;
            keep_visuals.push(path);
        }
        remove_others(&vdir, &keep_visuals);
        write_json(
            &pdir.join("page.json"),
            &PageFile {
                id: p.id.clone(),
                settings: p.settings.clone(),
                visuals: p.visuals.iter().map(|v| v.id.clone()).collect(),
            },
        )?;
        keep_pages.push(pdir);
    }
    // Pages that were deleted.
    if let Ok(entries) = std::fs::read_dir(&pages_dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() && !keep_pages.contains(&p) {
                let _ = std::fs::remove_dir_all(&p);
            }
        }
    }
    write_json(
        &dir.join("report.json"),
        &ReportFile {
            format: REPORT_FORMAT,
            pages: report.pages.iter().map(|p| p.id.clone()).collect(),
            filters: report.filters.clone(),
            diagram: report.diagram.clone(),
        },
    )
}

fn remove_others(dir: &Path, keep: &[PathBuf]) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "json") && !keep.contains(&p) {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
}
