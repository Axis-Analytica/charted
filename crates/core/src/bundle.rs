//! `.chartedz` bundles: a project folder in one zip file, for sending to someone.
//! The cache (loaded data, recovery, lock) stays out; it is rebuilt on open. Data files
//! inside the project folder travel with it; sources elsewhere keep their paths.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::project::CACHE_DIR;
use crate::session::CoreError;

pub const EXTENSION: &str = "chartedz";
/// Bundles larger than this aren't opened (a zip bomb guard).
const MAX_UNPACKED: u64 = 8 << 30;

fn invalid(m: impl Into<String>) -> CoreError {
    CoreError::Invalid(m.into())
}

/// Writes the project in `dir` to `out` (a .chartedz file).
pub fn write(dir: &Path, out: &Path) -> Result<(), CoreError> {
    let tmp = out.with_extension("chartedz.tmp");
    let file = std::fs::File::create(&tmp)
        .map_err(|e| invalid(format!("couldn't create {}: {e}", tmp.display())))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);
    let mut stack = vec![dir.to_path_buf()];
    let mut buf = Vec::new();
    while let Some(d) = stack.pop() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&d)
            .map_err(|e| invalid(format!("couldn't read {}: {e}", d.display())))?
            .flatten()
            .map(|e| e.path())
            .collect();
        entries.sort();
        for p in entries {
            let rel = p.strip_prefix(dir).unwrap_or(&p);
            let name = rel.to_string_lossy().replace('\\', "/");
            if name == CACHE_DIR
                || name.starts_with(&format!("{CACHE_DIR}/"))
                || name.starts_with(".git")
            {
                continue;
            }
            if p.is_dir() {
                stack.push(p);
            } else if p.is_file() {
                zip.start_file(&name, opts)
                    .map_err(|e| invalid(e.to_string()))?;
                buf.clear();
                std::fs::File::open(&p)
                    .and_then(|mut f| f.read_to_end(&mut buf))
                    .map_err(|e| invalid(format!("couldn't read {}: {e}", p.display())))?;
                zip.write_all(&buf).map_err(|e| invalid(e.to_string()))?;
            }
        }
    }
    zip.finish().map_err(|e| invalid(e.to_string()))?;
    std::fs::rename(&tmp, out)
        .map_err(|e| invalid(format!("couldn't write {}: {e}", out.display())))
}

/// Unpacks a bundle into a new folder under `parent` (named after the bundle) and
/// returns that folder.
pub fn unpack(bundle: &Path, parent: &Path) -> Result<PathBuf, CoreError> {
    let file = std::fs::File::open(bundle)
        .map_err(|e| invalid(format!("couldn't open {}: {e}", bundle.display())))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| invalid(format!("{} isn't a charted bundle: {e}", bundle.display())))?;
    let total: u64 = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|f| f.size()))
        .sum();
    if total > MAX_UNPACKED {
        return Err(invalid("the bundle unpacks to more than 8 GB"));
    }
    let has_manifest = zip
        .file_names()
        .flatten()
        .any(|n| !n.contains('/') && n.ends_with(&format!(".{}", crate::project::MANIFEST_EXT)));
    if !has_manifest {
        return Err(invalid(format!(
            "{} has no charted project in it",
            bundle.display()
        )));
    }
    let stem = bundle
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Project".into());
    let mut dir = parent.join(&stem);
    let mut k = 2;
    while dir.exists() {
        dir = parent.join(format!("{stem} {k}"));
        k += 1;
    }
    std::fs::create_dir_all(&dir)
        .map_err(|e| invalid(format!("couldn't create {}: {e}", dir.display())))?;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| invalid(e.to_string()))?;
        // Only paths inside the folder (no "..", no absolute paths).
        let Some(rel) = f.enclosed_name() else {
            continue;
        };
        let path = dir.join(rel);
        if f.is_dir() {
            std::fs::create_dir_all(&path).map_err(|e| invalid(e.to_string()))?;
            continue;
        }
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).map_err(|e| invalid(e.to_string()))?;
        }
        let mut out = std::fs::File::create(&path)
            .map_err(|e| invalid(format!("couldn't write {}: {e}", path.display())))?;
        std::io::copy(&mut f, &mut out).map_err(|e| invalid(e.to_string()))?;
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundles_round_trip_without_the_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("Shop");
        std::fs::create_dir_all(p.join(CACHE_DIR)).unwrap();
        std::fs::create_dir_all(p.join("data")).unwrap();
        std::fs::write(
            p.join(format!("Shop.{}", crate::project::MANIFEST_EXT)),
            "{}",
        )
        .unwrap();
        std::fs::write(p.join("data/a.csv"), "x\n1\n").unwrap();
        std::fs::write(p.join(CACHE_DIR).join("data.duckdb"), "big").unwrap();
        let b = tmp.path().join("Shop.chartedz");
        write(&p, &b).unwrap();
        let out = unpack(&b, &tmp.path().join("opened")).unwrap();
        assert_eq!(
            std::fs::read_to_string(out.join("data/a.csv")).unwrap(),
            "x\n1\n"
        );
        assert!(!out.join(CACHE_DIR).exists());
        // A second copy gets its own folder.
        assert_eq!(
            unpack(&b, &tmp.path().join("opened"))
                .unwrap()
                .file_name()
                .unwrap(),
            "Shop 2"
        );
        std::fs::write(tmp.path().join("bad.chartedz"), "nope").unwrap();
        assert!(unpack(&tmp.path().join("bad.chartedz"), tmp.path()).is_err());
    }
}
