//! Crash-safe file writes: write beside the target, flush to disk, then rename over it.
//! A crash mid-save leaves either the old file or the new one, never a torn mix.

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

pub fn write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    // Persist the rename itself.
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn replaces_contents_and_leaves_no_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.json");
        super::write(&p, b"one").unwrap();
        super::write(&p, b"two").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"two");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
