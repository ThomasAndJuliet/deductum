// SPDX-License-Identifier: GPL-3.0-only

//! `pam_cosmic_term_sudo.so`: lets sudo accept an approval signed by the
//! user's COSMIC Terminal sudo agent, instead of a password. Listed first in
//! sudo's PAM stack as `auth sufficient`, so that whatever does not end in a
//! good signature leaves sudo to its other methods. The logic is
//! `ct_sudo_protocol::authenticate`; this is its surroundings.
//!
//! It runs inside sudo, as root, and talks to a socket in a directory the
//! user controls, so before sending anything it checks that the path is the
//! user's own socket, not a link, and that what answers runs as the user.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ct_sudo_protocol::{
    Context, ModuleEnv, Outcome, Reply, TIMEOUT, authenticate, keys_path, read_reply, socket_path,
};

const PAM_SUCCESS: c_int = 0;
const PAM_AUTH_ERR: c_int = 7;
const PAM_IGNORE: c_int = 25;

#[allow(non_camel_case_types)]
type pam_handle_t = c_void;

unsafe extern "C" {
    fn pam_get_user(pamh: *mut pam_handle_t, user: *mut *const c_char, prompt: *const c_char) -> c_int;
}

struct Pam {
    handle: *mut pam_handle_t,
}

fn read_link(path: &str) -> Result<String, String> {
    std::fs::read_link(path)
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|err| format!("{path}: {err}"))
}

fn user_name(uid: u32) -> Result<String, String> {
    let mut buf = vec![0 as c_char; 16 << 10];
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: the buffers outlive the call, and are sized as passed
    let rc = unsafe { libc::getpwuid_r(uid, &mut pwd, buf.as_mut_ptr(), buf.len(), &mut found) };
    if rc != 0 || found.is_null() {
        return Err(format!("no user {uid}"));
    }
    // SAFETY: getpwuid_r succeeded, so pw_name points into buf
    Ok(unsafe { CStr::from_ptr(pwd.pw_name) }.to_string_lossy().into_owned())
}

/// Owned by root and writable by nobody else
fn root_only(path: &Path) -> Result<std::fs::Metadata, String> {
    let meta = std::fs::symlink_metadata(path).map_err(|err| format!("{}: {err}", path.display()))?;
    if meta.uid() != 0 || meta.permissions().mode() & 0o022 != 0 {
        return Err(format!("{} is not root's alone", path.display()));
    }
    Ok(meta)
}

impl ModuleEnv for Pam {
    fn context(&self) -> Result<Context, String> {
        // SAFETY: plain calls without arguments to keep alive
        let uid = unsafe { libc::getuid() };
        let mut target: *const c_char = std::ptr::null();
        // SAFETY: PAM owns the string it returns, for the handle's lifetime
        if unsafe { pam_get_user(self.handle, &mut target, std::ptr::null()) } != PAM_SUCCESS || target.is_null() {
            return Err("no user to authenticate".to_string());
        }
        // SAFETY: as above
        let target = unsafe { CStr::from_ptr(target) }.to_string_lossy().into_owned();
        let host = std::fs::read_to_string("/proc/sys/kernel/hostname")
            .map(|h| h.trim().to_string())
            .map_err(|err| err.to_string())?;
        let argv = std::fs::read("/proc/self/cmdline")
            .map_err(|err| err.to_string())?
            .split(|b| *b == 0)
            .filter(|arg| !arg.is_empty())
            .map(|arg| String::from_utf8_lossy(arg).into_owned())
            .collect();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        Ok(Context {
            host,
            user: user_name(uid)?,
            uid,
            target,
            cwd: read_link("/proc/self/cwd")?,
            argv,
            session: std::env::var("ZELLIJ_SESSION_NAME").ok(),
            cgroup: std::fs::read_to_string("/proc/self/cgroup")
                .map(|c| c.trim().to_string())
                .unwrap_or_default(),
            now,
        })
    }

    fn nonce(&self) -> Result<[u8; 32], String> {
        let mut nonce = [0u8; 32];
        // SAFETY: the buffer is 32 bytes, as passed
        let n = unsafe { libc::getrandom(nonce.as_mut_ptr().cast(), nonce.len(), 0) };
        if n != 32 {
            return Err("no randomness".to_string());
        }
        Ok(nonce)
    }

    fn authorized_keys(&self, user: &str) -> Result<Option<String>, String> {
        if user.is_empty() || user.contains('/') || user.starts_with('.') {
            return Err(format!("odd user name: {user}"));
        }
        let path = keys_path(user);
        if !path.exists() {
            return Ok(None);
        }
        for dir in path.ancestors().skip(1).take(2) {
            root_only(dir)?;
        }
        let meta = root_only(&path)?;
        if !meta.file_type().is_file() {
            return Err(format!("{} is not a file", path.display()));
        }
        std::fs::read_to_string(&path)
            .map(Some)
            .map_err(|err| format!("{}: {err}", path.display()))
    }

    fn exchange(&self, uid: u32, request: &[u8]) -> Result<Reply, String> {
        let path = socket_path(uid);
        let meta = std::fs::symlink_metadata(&path).map_err(|err| format!("no agent: {err}"))?;
        use std::os::unix::fs::FileTypeExt;
        if !meta.file_type().is_socket() || meta.uid() != uid {
            return Err(format!("{} is not the user's socket", path.display()));
        }
        let mut stream = UnixStream::connect(&path).map_err(|err| format!("no agent: {err}"))?;
        // Whatever answers must be the user's: the agent, or the sshd
        // forwarding to it
        let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: cred and len are sized for SO_PEERCRED
        let rc = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut cred as *mut libc::ucred).cast(),
                &mut len,
            )
        };
        if rc != 0 || cred.uid != uid {
            return Err("the socket is not the user's agent".to_string());
        }
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|err| err.to_string())?;
        stream.set_read_timeout(Some(TIMEOUT)).map_err(|err| err.to_string())?;
        std::io::Write::write_all(&mut stream, request).map_err(|err| err.to_string())?;
        let mut reader = (&mut stream).take(64 << 10);
        read_reply(&mut reader).map_err(|err| err.0)
    }
}

fn log(priority: c_int, message: &str) {
    let Ok(message) = CString::new(format!("pam_cosmic_term_sudo: {message}")) else {
        return;
    };
    // SAFETY: a fixed format with one string argument
    unsafe { libc::syslog(libc::LOG_AUTHPRIV | priority, c"%s".as_ptr(), message.as_ptr()) };
}

/// # Safety
/// Called by PAM with a live handle
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pam_sm_authenticate(
    pamh: *mut pam_handle_t,
    _flags: c_int,
    _argc: c_int,
    _argv: *const *const c_char,
) -> c_int {
    // Never unwind into sudo
    let outcome = std::panic::catch_unwind(|| authenticate(&Pam { handle: pamh }))
        .unwrap_or_else(|_| Outcome::Ignored("the module failed".to_string()));
    match outcome {
        Outcome::Accepted => {
            log(libc::LOG_NOTICE, "approved by the user's terminal");
            PAM_SUCCESS
        }
        Outcome::Rejected(reason) => {
            log(libc::LOG_NOTICE, &format!("not approved: {reason}"));
            PAM_AUTH_ERR
        }
        Outcome::Ignored(reason) => {
            // Wire errors must be visible at the same journal level as
            // refusals, rather than disappearing as debug-only fallback.
            let priority = if reason.contains("sudo protocol") || reason.contains("sudo frame") {
                libc::LOG_NOTICE
            } else {
                libc::LOG_DEBUG
            };
            log(priority, &reason);
            PAM_IGNORE
        }
    }
}

/// # Safety
/// Called by PAM with a live handle
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pam_sm_setcred(
    _pamh: *mut pam_handle_t,
    _flags: c_int,
    _argc: c_int,
    _argv: *const *const c_char,
) -> c_int {
    PAM_IGNORE
}
