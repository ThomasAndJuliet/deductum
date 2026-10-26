// SPDX-License-Identifier: GPL-3.0-only

//! Binding the agent's own sockets without deleting a live listener or an
//! unrelated file. Socket-activated agents keep systemd's descriptors.

use std::{
    fs::{self, OpenOptions},
    io,
    os::{
        fd::AsRawFd,
        unix::{
            fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::Path,
};

pub fn listen(path: &Path) -> io::Result<UnixListener> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Serialize standalone agents through bind, not just through unlink.
    let mut lock_path = path.as_os_str().to_os_string();
    lock_path.push(".lock");
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(lock_path)?;
    // SAFETY: the file descriptor belongs to the live lock file above.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(io::Error::last_os_error());
    }
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            // SAFETY: getuid has no arguments or side effects.
            if !meta.file_type().is_socket() || meta.uid() != unsafe { libc::getuid() } {
                return Err(io::Error::other(format!(
                    "{} is not this user's socket",
                    path.display()
                )));
            }
            match UnixStream::connect(path) {
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::AddrInUse,
                        "a live sudo listener already owns this path",
                    ));
                }
                Err(err) if err.kind() == io::ErrorKind::ConnectionRefused => {
                    let current = fs::symlink_metadata(path)?;
                    if current.dev() != meta.dev() || current.ino() != meta.ino() {
                        return Err(io::Error::other(
                            "the sudo socket changed while checking it",
                        ));
                    }
                    fs::remove_file(path)?;
                }
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(err),
            }
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_dead_bindings_but_preserves_live_sockets_and_files() {
        const DEAD_SOCKET: &str = "CT_TEST_DEAD_SUDO_SOCKET";
        if let Some(path) = std::env::var_os(DEAD_SOCKET) {
            drop(UnixListener::bind(path).unwrap());
            return;
        }
        let dir = std::env::temp_dir().join(format!("ct-sudo-socket-{}", crate::random_hex(16)));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("agent.sock");
        // A concurrent test can fork while this process holds a listener and
        // keep its descriptor alive after drop. Make the stale binding in an
        // isolated test process and wait for every descriptor there to close.
        assert!(std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "sudo_socket::tests::replaces_dead_bindings_but_preserves_live_sockets_and_files", "--quiet"])
            .env(DEAD_SOCKET, &path)
            .status().unwrap().success());
        let listener = listen(&path).unwrap();
        let inode = fs::metadata(&path).unwrap().ino();
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        assert_eq!(listen(&path).unwrap_err().kind(), io::ErrorKind::AddrInUse);
        assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
        drop(listener);
        fs::remove_file(&path).unwrap();
        fs::write(&path, "keep me").unwrap();
        assert!(listen(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep me");
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink("missing", &path).unwrap();
        assert!(listen(&path).is_err());
        assert!(
            fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
