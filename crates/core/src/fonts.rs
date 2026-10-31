//! Fonts the user adds: a library of font files in charted's state folder, listed for
//! every report, and a copy in the project's `fonts/` folder of each one a report uses,
//! so the report looks the same on another machine. The UI loads the files itself
//! (`DataRequest::ReadFont`) and registers them as web fonts.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::session::CoreError;

pub const PROJECT_DIR: &str = "fonts";
const EXTENSIONS: &[&str] = &["ttf", "otf", "woff2"];
const MAX_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FontFile {
    pub family: String,
    /// The file's name, in the library or the project's fonts folder.
    pub file: String,
    pub weight: u16,
    pub italic: bool,
    /// Where it is: the user's library, the open project, or both.
    pub in_library: bool,
    pub in_project: bool,
}

pub fn library_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("fonts")
}

fn invalid(m: impl Into<String>) -> CoreError {
    CoreError::Invalid(m.into())
}

fn is_font(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// The family, weight and style a font file declares. A WOFF2 file is compressed, so its
/// family comes from its name ("Brand Sans-Bold.woff2" → "Brand Sans", bold).
fn describe(p: &Path) -> Option<(String, u16, bool)> {
    let stem = p.file_stem()?.to_string_lossy().to_string();
    let data = std::fs::read(p).ok()?;
    if let Ok(face) = ttf_parser::Face::parse(&data, 0) {
        let name = |id| {
            face.names()
                .into_iter()
                .filter(|n| n.name_id == id)
                .find_map(|n| n.to_string())
        };
        let family = name(ttf_parser::name_id::TYPOGRAPHIC_FAMILY)
            .or_else(|| name(ttf_parser::name_id::FAMILY))?;
        return Some((family, face.weight().to_number(), face.is_italic()));
    }
    if !stem.is_empty() && data.starts_with(b"wOF2") {
        let (family, style) = stem.split_once('-').unwrap_or((&stem, ""));
        let style = style.to_ascii_lowercase();
        let weight = if style.contains("bold") { 700 } else { 400 };
        return Some((family.trim().to_string(), weight, style.contains("italic")));
    }
    None
}

fn scan(dir: &Path) -> Vec<(String, (String, u16, bool))> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut out: Vec<_> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_font(p))
        .filter_map(|p| Some((p.file_name()?.to_string_lossy().to_string(), describe(&p)?)))
        .collect();
    out.sort();
    out
}

/// The library's fonts and the open project's, by file.
pub fn list(state_dir: &Path, project: Option<&Path>) -> Vec<FontFile> {
    let mut out: Vec<FontFile> = scan(&library_dir(state_dir))
        .into_iter()
        .map(|(file, (family, weight, italic))| FontFile {
            family,
            file,
            weight,
            italic,
            in_library: true,
            in_project: false,
        })
        .collect();
    for (file, (family, weight, italic)) in project.map(|p| scan(&p.join(PROJECT_DIR))).unwrap_or_default() {
        match out.iter_mut().find(|f| f.file == file) {
            Some(f) => f.in_project = true,
            None => out.push(FontFile {
                family,
                file,
                weight,
                italic,
                in_library: false,
                in_project: true,
            }),
        }
    }
    out
}

/// Copies a font file into the library; returns its family.
pub fn add(state_dir: &Path, path: &Path) -> Result<String, CoreError> {
    if !is_font(path) {
        return Err(invalid("choose a font file: TTF, OTF or WOFF2"));
    }
    let size = std::fs::metadata(path)
        .map_err(|e| invalid(format!("couldn't read {}: {e}", path.display())))?
        .len();
    if size > MAX_BYTES {
        return Err(invalid("that font file is over 20 MB"));
    }
    let (family, ..) = describe(path).ok_or_else(|| {
        invalid(format!("{} isn't a font charted can read", path.display()))
    })?;
    let dir = library_dir(state_dir);
    std::fs::create_dir_all(&dir).map_err(|e| invalid(format!("couldn't create {}: {e}", dir.display())))?;
    let name = path.file_name().ok_or_else(|| invalid("no file name"))?;
    std::fs::copy(path, dir.join(name)).map_err(|e| invalid(format!("couldn't copy the font: {e}")))?;
    Ok(family)
}

/// A file name from the UI, kept to a plain name in its folder.
fn file_in(dir: &Path, file: &str) -> Result<PathBuf, CoreError> {
    if file.is_empty() || file.contains(['/', '\\']) || file.starts_with('.') {
        return Err(invalid(format!("{file} isn't a font file name")));
    }
    let p = dir.join(file);
    if !is_font(&p) {
        return Err(invalid(format!("{file} isn't a font file")));
    }
    Ok(p)
}

pub fn remove(state_dir: &Path, file: &str) -> Result<(), CoreError> {
    let p = file_in(&library_dir(state_dir), file)?;
    std::fs::remove_file(&p).map_err(|e| invalid(format!("couldn't remove {file}: {e}")))
}

/// A font's bytes, from the project's fonts folder first, then the library.
pub fn read(state_dir: &Path, project: Option<&Path>, file: &str) -> Result<Vec<u8>, CoreError> {
    let mut dirs: Vec<PathBuf> = project.map(|p| p.join(PROJECT_DIR)).into_iter().collect();
    dirs.push(library_dir(state_dir));
    for d in dirs {
        let p = file_in(&d, file)?;
        if p.is_file() {
            return std::fs::read(&p).map_err(|e| invalid(format!("couldn't read {file}: {e}")));
        }
    }
    Err(invalid(format!("there's no font file {file}")))
}

/// The font families a report names: theme text classes (`fontFace`) and visuals'
/// `fontFamily` properties, each of which may be a CSS-style list.
pub fn used(report: &serde_json::Value) -> Vec<String> {
    fn walk(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::Object(o) => {
                for (k, x) in o {
                    match (k.as_str(), x.as_str()) {
                        ("fontFace" | "fontFamily", Some(s)) => out.extend(
                            s.split(',')
                                .map(|f| f.trim().trim_matches(['\'', '"']).to_string())
                                .filter(|f| !f.is_empty()),
                        ),
                        _ => walk(x, out),
                    }
                }
            }
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    let mut out = vec![];
    walk(report, &mut out);
    out.sort();
    out.dedup();
    out
}

/// Copies each library font the report uses into the project's fonts folder.
pub fn bundle(state_dir: &Path, project: &Path, report: &serde_json::Value) -> Result<(), CoreError> {
    let used = used(report);
    if used.is_empty() {
        return Ok(());
    }
    let to = project.join(PROJECT_DIR);
    for f in list(state_dir, Some(project)) {
        if f.in_library && !f.in_project && used.iter().any(|u| u.eq_ignore_ascii_case(&f.family)) {
            std::fs::create_dir_all(&to).map_err(|e| invalid(format!("couldn't create {}: {e}", to.display())))?;
            std::fs::copy(library_dir(state_dir).join(&f.file), to.join(&f.file))
                .map_err(|e| invalid(format!("couldn't copy {} into the project: {e}", f.file)))?;
        }
    }
    Ok(())
}
