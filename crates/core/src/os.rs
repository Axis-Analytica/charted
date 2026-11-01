//! What differs between Linux, macOS and Windows: the home folder, finding a program on
//! PATH, and whether a process is still running.

use std::path::PathBuf;

/// The user's home folder (`$HOME`, or the profile folder on Windows).
pub fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

/// `std::fs::canonicalize`, but plain `C:\…` paths on Windows rather than `\\?\C:\…`.
pub fn canonicalize(path: &std::path::Path) -> std::io::Result<PathBuf> {
    dunce::canonicalize(path)
}

/// Where charted keeps its settings, recent files and crash records:
/// `$XDG_STATE_HOME/charted` (`~/.local/state/charted`) on Linux, the local application
/// data folder elsewhere.
pub fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(dirs::state_dir)
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("charted")
}

/// `name` on PATH, as `which` finds it. On Windows also `name.exe`, `name.cmd` and the
/// rest of PATHEXT, which is how npm installs agents there.
pub fn find_program(name: &str) -> Option<PathBuf> {
    find_in(&std::env::var_os("PATH")?, name)
}

fn find_in(path: &std::ffi::OsStr, name: &str) -> Option<PathBuf> {
    let exts: Vec<String> = if cfg!(windows) {
        let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        std::iter::once(String::new())
            .chain(
                pathext
                    .split(';')
                    .filter(|e| !e.is_empty())
                    .map(str::to_lowercase),
            )
            .collect()
    } else {
        vec![String::new()]
    };
    std::env::split_paths(path).find_map(|d| {
        exts.iter()
            .map(|e| d.join(format!("{name}{e}")))
            .find(|p| p.is_file())
    })
}

/// The script an npm `.cmd` shim runs with node (`"%dp0%\\node_modules\\…\\cli.js"`),
/// resolved against the shim's folder. Running `node script` directly avoids cmd.exe,
/// which can't pass arguments with line breaks through a batch file.
pub fn npm_shim_script(shim: &str, shim_dir: &std::path::Path) -> Option<PathBuf> {
    let start = shim.rfind("\"%dp0%\\")? + "\"%dp0%\\".len();
    let end = start + shim[start..].find('"')?;
    let rel = &shim[start..end];
    [".js", ".mjs", ".cjs"]
        .iter()
        .any(|e| rel.ends_with(e))
        .then(|| shim_dir.join(rel.replace('\\', std::path::MAIN_SEPARATOR_STR)))
}

/// Whether the process `pid` is running.
#[cfg(target_os = "linux")]
pub fn pid_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

#[cfg(all(unix, not(target_os = "linux")))]
pub fn pid_alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    // SAFETY: signal 0 only checks that the process exists. EPERM means it exists but
    // belongs to someone else.
    unsafe {
        libc::kill(pid, 0) == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
}

#[cfg(windows)]
pub fn pid_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: the handle is checked and closed; the exit code is written to a local.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return false;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code) != 0;
        CloseHandle(h);
        ok && code == STILL_ACTIVE as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_is_alive() {
        assert!(pid_alive(std::process::id()));
        assert!(!pid_alive(u32::MAX - 1));
    }

    #[test]
    fn reads_npm_shims() {
        let shim = "@ECHO off\r\nIF EXIST \"%dp0%\\node.exe\" (\r\n  SET \"_prog=%dp0%\\node.exe\"\r\n)\r\nendLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & \"%_prog%\"  \"%dp0%\\node_modules\\@openai\\codex\\bin\\codex.js\" %*\r\n";
        let dir = std::path::Path::new("npm");
        let sep = std::path::MAIN_SEPARATOR_STR;
        assert_eq!(
            npm_shim_script(shim, dir),
            Some(dir.join(["node_modules", "@openai", "codex", "bin", "codex.js"].join(sep)))
        );
        assert_eq!(
            npm_shim_script("@echo off\r\n\"%dp0%\\tool.exe\" %*", dir),
            None
        );
    }

    #[test]
    fn finds_programs_on_path() {
        let dir = tempfile::tempdir().unwrap();
        let name = if cfg!(windows) {
            "fake-agent.cmd"
        } else {
            "fake-agent"
        };
        std::fs::write(dir.path().join(name), "").unwrap();
        let found = find_in(dir.path().as_os_str(), "fake-agent");
        let missing = find_in(dir.path().as_os_str(), "not-an-agent");
        assert_eq!(found, Some(dir.path().join(name)));
        assert_eq!(missing, None);
    }
}
