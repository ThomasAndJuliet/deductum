// SPDX-License-Identifier: GPL-3.0-only

//! Moving agent conversations and their worktrees between computers.
//!
//! The terminal and the `ct-transfer` helper share this library. Each step of
//! a transfer happens on one computer, so it is a [`helper::Request`] that the
//! helper answers there, locally or over ssh; [`transfer`] runs the steps in
//! order from whichever computer the terminal is on. The design is in
//! `AGENTS-PLAN.md`, under "Transferring sessions".

pub mod b64;
pub mod helper;
pub mod mount_setup;
pub mod project;
pub mod provider;
pub mod sudo_control;
pub mod sudo_socket;
pub mod transfer;
pub mod vault;
pub mod workspace;

use std::path::PathBuf;

/// Where a computer keeps the files this library owns
#[derive(Clone, Debug)]
pub struct Env {
    pub home: PathBuf,
    /// `$XDG_STATE_HOME/cosmic-term`
    pub state_dir: PathBuf,
}

impl Env {
    /// The current user's, from the process environment
    pub fn from_process() -> Self {
        let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
        let state = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| home.join(".local/state"));
        Self {
            state_dir: state.join("cosmic-term"),
            home,
        }
    }

    /// An environment rooted at `home`, as in tests
    pub fn at(home: impl Into<PathBuf>) -> Self {
        let home = home.into();
        Self {
            state_dir: home.join(".local/state/cosmic-term"),
            home,
        }
    }
}

/// A random lowercase hex string of `len` characters
pub fn random_hex(len: usize) -> String {
    use std::hash::{BuildHasher, Hasher, RandomState};
    let mut out = String::with_capacity(len + 16);
    while out.len() < len {
        // Each RandomState is freshly keyed, so this needs no counter
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
        );
        hasher.write_u32(std::process::id());
        out.push_str(&format!("{:016x}", hasher.finish()));
    }
    out.truncate(len);
    out
}

/// A random version 4 UUID, as Claude names conversations
pub fn random_uuid() -> String {
    let hex = random_hex(32);
    let variant = ["8", "9", "a", "b"][usize::from(hex.as_bytes()[16] % 4)];
    format!(
        "{}-{}-4{}-{}{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[13..16],
        variant,
        &hex[17..20],
        &hex[20..32]
    )
}

/// Seconds since the epoch
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuids_look_like_version_4() {
        let id = random_uuid();
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(
            parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
            [8, 4, 4, 4, 12]
        );
        assert!(parts[2].starts_with('4'));
        assert!("89ab".contains(&parts[3][..1]));
        assert_ne!(random_uuid(), id);
    }
}
