//! The control socket's transport. Where there are Unix sockets it is one in the runtime
//! directory, readable only by this user; on Windows it is a loopback TCP port. Either
//! way a connection must first present a token issued for one request, so another
//! program can't drive the window by connecting.

use std::io;
use std::path::Path;

#[cfg(unix)]
pub type Stream = std::os::unix::net::UnixStream;
#[cfg(not(unix))]
pub type Stream = std::net::TcpStream;

pub struct Listener {
    #[cfg(unix)]
    inner: std::os::unix::net::UnixListener,
    #[cfg(not(unix))]
    inner: std::net::TcpListener,
    /// What `charted mcp --socket` connects to: the socket's path, or `127.0.0.1:port`.
    pub address: String,
}

impl Listener {
    /// Listens for this process, in `dir` (made private to the user) where sockets are
    /// files; clears sockets left by charted windows that have gone.
    #[cfg(unix)]
    pub fn bind(dir: &Path) -> io::Result<Listener> {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        for e in std::fs::read_dir(dir)?.flatten() {
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
        let inner = std::os::unix::net::UnixListener::bind(&socket)?;
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
        Ok(Listener {
            inner,
            address: socket.display().to_string(),
        })
    }

    #[cfg(not(unix))]
    pub fn bind(_dir: &Path) -> io::Result<Listener> {
        let inner = std::net::TcpListener::bind(("127.0.0.1", 0))?;
        let address = inner.local_addr()?.to_string();
        Ok(Listener { inner, address })
    }

    pub fn incoming(&self) -> impl Iterator<Item = Stream> + '_ {
        self.inner.incoming().flatten()
    }
}

/// Removes the socket file, if the transport has one (the app is closing).
pub fn remove(address: &str) {
    if cfg!(unix) {
        let _ = std::fs::remove_file(address);
    }
}

pub fn connect(address: &str) -> io::Result<Stream> {
    Stream::connect(address)
}

#[cfg(unix)]
fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    unsafe { libc::kill(pid, 0) == 0 }
}
