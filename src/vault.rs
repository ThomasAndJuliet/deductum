// SPDX-License-Identifier: GPL-3.0-only

//! The terminal's secrets store: saved passwords and the keys that approve
//! sudo, in one file that only `ct-sudo-agent` reads (see `AGENTS-PLAN.md`,
//! "Secrets and sudo approvals").
//!
//! The file is the magic, the Argon2id parameters and salt, a nonce, and the
//! entries as JSON sealed with XChaCha20-Poly1305 under the key Argon2id
//! derives from the store password. A wrong password fails to open it; a
//! changed byte fails authentication. Every save uses a fresh nonce. The
//! open store's key and contents are wiped from memory when dropped.

use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, AeadCore, KeyInit, OsRng, rand_core::RngCore},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

const MAGIC: &[u8] = b"cosmic-term vault 1\n";
const SALT: usize = 16;
const NONCE: usize = 24;

/// Argon2id's costs: memory in KiB, passes, lanes
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Params {
    pub memory: u32,
    pub passes: u32,
    pub lanes: u32,
}

impl Default for Params {
    /// About half a second on a laptop
    fn default() -> Self {
        Self {
            memory: 64 << 10,
            passes: 3,
            lanes: 1,
        }
    }
}

/// A saved secret
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Entry {
    pub id: u64,
    /// The machine it is for, by host name, or none for Other
    pub host: Option<String>,
    /// The account on that machine, or the name of an Other entry
    pub label: String,
    pub note: String,
    pub secret: String,
    /// Ask for the store password each time it is used
    pub always_ask: bool,
}

/// What the store holds; its secrets are wiped when it is dropped
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Contents {
    pub entries: Vec<Entry>,
    /// Each machine's sudo key, by host name, as an OpenSSH private key
    pub sudo_keys: BTreeMap<String, String>,
    pub next_id: u64,
}

impl Drop for Contents {
    fn drop(&mut self) {
        for entry in &mut self.entries {
            entry.secret.zeroize();
        }
        for key in self.sudo_keys.values_mut() {
            key.zeroize();
        }
    }
}

impl Contents {
    pub fn add(&mut self, mut entry: Entry) -> u64 {
        self.next_id += 1;
        entry.id = self.next_id;
        self.entries.push(entry);
        self.next_id
    }

    pub fn entry(&self, id: u64) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }
}

/// The key the store is sealed with, and how it was derived
#[derive(Clone, ZeroizeOnDrop)]
pub struct Key {
    key: [u8; 32],
    #[zeroize(skip)]
    salt: [u8; SALT],
    #[zeroize(skip)]
    params: Params,
}

impl Key {
    /// A new salt and key from a password, as when the store is made
    pub fn new(password: &str, params: Params) -> Result<Self, String> {
        let mut salt = [0u8; SALT];
        OsRng.fill_bytes(&mut salt);
        Self::derive(password, salt, params)
    }

    fn derive(password: &str, salt: [u8; SALT], params: Params) -> Result<Self, String> {
        let argon = argon2::Argon2::new(
            argon2::Algorithm::Argon2id,
            argon2::Version::V0x13,
            argon2::Params::new(params.memory, params.passes, params.lanes, Some(32))
                .map_err(|err| err.to_string())?,
        );
        let mut key = [0u8; 32];
        argon
            .hash_password_into(password.as_bytes(), &salt, &mut key)
            .map_err(|err| err.to_string())?;
        Ok(Self { key, salt, params })
    }

    /// Whether this is the key a password gives
    pub fn matches(&self, password: &str) -> bool {
        Self::derive(password, self.salt, self.params).is_ok_and(|other| {
            // Constant time, as the comparison of secrets should be
            other.key.iter().zip(self.key.iter()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
        })
    }
}

fn header(params: Params, salt: &[u8; SALT]) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    for n in [params.memory, params.passes, params.lanes] {
        out.extend_from_slice(&n.to_be_bytes());
    }
    out.extend_from_slice(salt);
    out
}

/// The store's file
pub fn seal(contents: &Contents, key: &Key) -> Result<Vec<u8>, String> {
    let cipher = XChaCha20Poly1305::new((&key.key).into());
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let plain = Zeroizing::new(serde_json::to_vec(contents).map_err(|err| err.to_string())?);
    let mut out = header(key.params, &key.salt);
    // The header is authenticated too, so its costs cannot be swapped
    let sealed = cipher
        .encrypt(
            &nonce,
            chacha20poly1305::aead::Payload {
                msg: &plain,
                aad: &out,
            },
        )
        .map_err(|_| "cannot seal the store".to_string())?;
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&sealed);
    Ok(out)
}

/// The store's contents and key, given its password
pub fn open(file: &[u8], password: &str) -> Result<(Contents, Key), String> {
    let rest = file
        .strip_prefix(MAGIC)
        .ok_or("not a secrets store")?;
    if rest.len() < 12 + SALT + NONCE {
        return Err("the store is cut short".to_string());
    }
    let n = |i: usize| u32::from_be_bytes(rest[i * 4..i * 4 + 4].try_into().unwrap());
    let params = Params {
        memory: n(0),
        passes: n(1),
        lanes: n(2),
    };
    // Refuse costs that would take the agent down, from a damaged file
    if params.memory > 4 << 20 || params.passes > 64 || params.lanes > 16 {
        return Err("the store's parameters are out of range".to_string());
    }
    let salt: [u8; SALT] = rest[12..12 + SALT].try_into().unwrap();
    let nonce = XNonce::from_slice(&rest[12 + SALT..12 + SALT + NONCE]);
    let sealed = &rest[12 + SALT + NONCE..];
    let key = Key::derive(password, salt, params)?;
    let cipher = XChaCha20Poly1305::new((&key.key).into());
    let aad = header(params, &salt);
    let plain = Zeroizing::new(
        cipher
            .decrypt(nonce, chacha20poly1305::aead::Payload { msg: sealed, aad: &aad })
            .map_err(|_| "the password is not right".to_string())?,
    );
    let contents = serde_json::from_slice(&plain).map_err(|err| err.to_string())?;
    Ok((contents, key))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cheap costs, for tests
    const FAST: Params = Params {
        memory: 64,
        passes: 1,
        lanes: 1,
    };

    #[test]
    fn a_store_opens_only_with_its_password_and_resists_tampering() {
        let mut contents = Contents::default();
        let id = contents.add(Entry {
            host: Some("neo".into()),
            label: "twright".into(),
            secret: "hunter2".into(),
            ..Default::default()
        });
        contents.sudo_keys.insert("neo".into(), "KEY".into());
        let key = Key::new("pw", FAST).unwrap();
        let file = seal(&contents, &key).unwrap();
        assert!(!file.windows(7).any(|w| w == b"hunter2"));

        let (opened, again) = open(&file, "pw").unwrap();
        assert_eq!(opened.entry(id).unwrap().secret, "hunter2");
        assert_eq!(opened.sudo_keys["neo"], "KEY");
        assert!(again.matches("pw") && !again.matches("other"));
        assert!(open(&file, "wrong").is_err());

        // Any changed byte, in the header or the sealed part, fails
        for i in [MAGIC.len() + 1, file.len() - 1] {
            let mut damaged = file.clone();
            damaged[i] ^= 1;
            assert!(open(&damaged, "pw").is_err());
        }
        // A fresh nonce each time
        assert_ne!(seal(&contents, &key).unwrap(), file);
    }
}
