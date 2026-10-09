// SPDX-License-Identifier: GPL-3.0-only

//! What COSMIC Terminal's sudo agent and its PAM module say to each other.
//!
//! An agent's tool call runs `sudo` without a terminal, so sudo cannot ask
//! for a password. The PAM module, first in sudo's PAM stack as `auth
//! sufficient`, builds a [`Challenge`] instead: what sudo is about to run,
//! where and for whom, with a fresh nonce. It sends it to the user's
//! `ct-sudo-agent` at [`socket_path`] — on another machine, a socket the
//! terminal forwards there over ssh — and accepts only an SSHSIG signature
//! over exactly those bytes, in the [`NAMESPACE`], by a key listed in the
//! root-owned [`KEYS_DIR`]`/<user>.pub`. The user approves in the terminal;
//! no password is ever sent. Anything other than a good signature leaves
//! sudo to its other methods.
//!
//! [`authenticate`] is the module's whole logic, against a [`ModuleEnv`], so
//! that it can be tested without PAM or root. The `sign` feature adds what
//! the agent needs ([`keys`]).

use std::io::{self, Read, Write};

/// The key crate, for the agent's use of the keys it returns
#[cfg(feature = "verify")]
pub use ssh_key;
use std::path::PathBuf;
use std::time::Duration;

/// The agent's socket, in each user's runtime directory
pub const SOCKET: &str = "cosmic-term-sudo.sock";

/// Where the module finds a user's public keys: one file per user, owned by
/// root and writable only by root
pub const KEYS_DIR: &str = "/etc/cosmic-term/sudo-keys";

/// The SSHSIG namespace, so that no other use of the key can be passed off
/// as an approval
pub const NAMESPACE: &str = "sudo@cosmic-term";

/// This protocol's version, sent in every request
pub const VERSION: u8 = 1;

/// The module's version, in each challenge, so the terminal can tell when a
/// machine's module is out of date
pub const MODULE_VERSION: u32 = 1;

/// How long the module waits for the user's answer
pub const TIMEOUT: Duration = Duration::from_secs(120);

const MAGIC: &[u8] = b"cosmic-term sudo challenge 1\n";
const MAX_CHALLENGE: usize = 64 << 10;
const MAX_REPLY: usize = 16 << 10;
const MAX_FIELD: usize = 32 << 10;

/// The agent's socket for a user
pub fn socket_path(uid: u32) -> PathBuf {
    PathBuf::from(format!("/run/user/{uid}/{SOCKET}"))
}

/// A user's key file
pub fn keys_path(user: &str) -> PathBuf {
    PathBuf::from(KEYS_DIR).join(format!("{user}.pub"))
}

#[derive(Debug, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Self(err.to_string())
    }
}

fn error(message: impl Into<String>) -> Error {
    Error(message.into())
}

/// What the module asks the agent to sign. Everything in it but `session`
/// is read by the module inside sudo, as root, so it is what sudo will do;
/// `session` comes from sudo's environment, which the caller controls, and
/// only says where to show the request
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Challenge {
    pub nonce: [u8; 32],
    /// The machine's host name
    pub host: String,
    /// Who runs sudo
    pub user: String,
    /// Who they want to become
    pub target: String,
    pub cwd: String,
    /// sudo's command line: the command it runs, with sudo's own options
    pub argv: Vec<String>,
    /// The Zellij session that ran sudo, if any
    pub session: Option<String>,
    /// sudo's cgroup
    pub cgroup: String,
    /// Seconds since the epoch, on the machine
    pub issued_at: u64,
    pub module_version: u32,
}

fn put(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if self.0.len() < n {
            return Err(error("the challenge is cut short"));
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }

    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn bytes(&mut self) -> Result<&'a [u8], Error> {
        let n = self.u32()? as usize;
        if n > MAX_FIELD {
            return Err(error("a field of the challenge is too long"));
        }
        self.take(n)
    }

    fn string(&mut self) -> Result<String, Error> {
        String::from_utf8(self.bytes()?.to_vec()).map_err(|_| error("a field is not UTF-8"))
    }
}

impl Challenge {
    /// The bytes that are signed: the magic, then each field length-prefixed
    pub fn encode(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        put(&mut out, &self.nonce);
        put(&mut out, self.host.as_bytes());
        put(&mut out, self.user.as_bytes());
        put(&mut out, self.target.as_bytes());
        put(&mut out, self.cwd.as_bytes());
        out.extend_from_slice(&(self.argv.len() as u32).to_be_bytes());
        for arg in &self.argv {
            put(&mut out, arg.as_bytes());
        }
        put(&mut out, self.session.as_deref().unwrap_or_default().as_bytes());
        put(&mut out, self.cgroup.as_bytes());
        out.extend_from_slice(&self.issued_at.to_be_bytes());
        out.extend_from_slice(&self.module_version.to_be_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let rest = bytes
            .strip_prefix(MAGIC)
            .ok_or_else(|| error("not a challenge"))?;
        let mut r = Reader(rest);
        let nonce = r
            .bytes()?
            .try_into()
            .map_err(|_| error("the nonce is not 32 bytes"))?;
        let host = r.string()?;
        let user = r.string()?;
        let target = r.string()?;
        let cwd = r.string()?;
        let count = r.u32()? as usize;
        if count > 4096 {
            return Err(error("too many arguments"));
        }
        let argv = (0..count).map(|_| r.string()).collect::<Result<_, _>>()?;
        let session = Some(r.string()?).filter(|s| !s.is_empty());
        let cgroup = r.string()?;
        let issued_at = u64::from_be_bytes(r.take(8)?.try_into().unwrap());
        let module_version = r.u32()?;
        if !r.0.is_empty() {
            return Err(error("the challenge has trailing bytes"));
        }
        Ok(Self {
            nonce,
            host,
            user,
            target,
            cwd,
            argv,
            session,
            cgroup,
            issued_at,
            module_version,
        })
    }

    /// The command sudo will run, without sudo and its own options, for
    /// showing
    pub fn command(&self) -> Vec<&str> {
        let mut args = self.argv.iter().map(String::as_str);
        // The program itself
        args.next();
        let mut rest: Vec<&str> = args.collect();
        // sudo's options before the command; the ones taking a value
        let with_value = ["-u", "-g", "-C", "-D", "-h", "-p", "-r", "-t", "-T", "-U"];
        let mut i = 0;
        while i < rest.len() && rest[i].starts_with('-') {
            if rest[i] == "--" {
                i += 1;
                break;
            }
            i += if with_value.contains(&rest[i]) { 2 } else { 1 };
        }
        rest.drain(..i.min(rest.len()));
        rest
    }
}

/// The agent's answer
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    /// An SSHSIG signature over the challenge, PEM-armoured
    Signed(String),
    /// Why not
    Refused(String),
}

/// Write a request: the version, then the challenge
pub fn write_request(out: &mut impl Write, challenge: &[u8]) -> Result<(), Error> {
    let mut frame = vec![VERSION];
    put(&mut frame, challenge);
    out.write_all(&frame)?;
    out.flush()?;
    Ok(())
}

fn read_frame(input: &mut impl Read, max: usize) -> Result<(u8, Vec<u8>), Error> {
    let mut head = [0u8; 5];
    input.read_exact(&mut head)?;
    let len = u32::from_be_bytes(head[1..].try_into().unwrap()) as usize;
    if len > max {
        return Err(error("the message is too long"));
    }
    let mut body = vec![0u8; len];
    input.read_exact(&mut body)?;
    Ok((head[0], body))
}

/// Read a request, returning the challenge's bytes
pub fn read_request(input: &mut impl Read) -> Result<Vec<u8>, Error> {
    let (version, body) = read_frame(input, MAX_CHALLENGE)?;
    if version != VERSION {
        return Err(error(format!("protocol version {version}, not {VERSION}")));
    }
    Ok(body)
}

pub fn write_reply(out: &mut impl Write, reply: &Reply) -> Result<(), Error> {
    let (tag, body) = match reply {
        Reply::Refused(reason) => (0u8, reason.as_bytes()),
        Reply::Signed(signature) => (1u8, signature.as_bytes()),
    };
    let mut frame = vec![tag];
    put(&mut frame, body);
    out.write_all(&frame)?;
    out.flush()?;
    Ok(())
}

pub fn read_reply(input: &mut impl Read) -> Result<Reply, Error> {
    let (tag, body) = read_frame(input, MAX_REPLY)?;
    let text = String::from_utf8(body).map_err(|_| error("the reply is not UTF-8"))?;
    match tag {
        0 => Ok(Reply::Refused(text)),
        1 => Ok(Reply::Signed(text)),
        _ => Err(error("an unknown reply")),
    }
}

/// What the module knows of the sudo it runs in
#[derive(Clone, Debug)]
pub struct Context {
    pub host: String,
    pub user: String,
    pub uid: u32,
    pub target: String,
    pub cwd: String,
    pub argv: Vec<String>,
    pub session: Option<String>,
    pub cgroup: String,
    pub now: u64,
}

/// The module's surroundings: PAM, the system, and the socket
pub trait ModuleEnv {
    fn context(&self) -> Result<Context, String>;
    fn nonce(&self) -> Result<[u8; 32], String>;
    /// The contents of the user's key file, or `None` if they have none. The
    /// environment checks the file is root's and writable only by root
    fn authorized_keys(&self, user: &str) -> Result<Option<String>, String>;
    /// Send a request to the user's agent and return its reply, within
    /// [`TIMEOUT`]
    fn exchange(&self, uid: u32, request: &[u8]) -> Result<Reply, String>;
}

/// What the module tells PAM
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// A good signature: sudo may go ahead
    Accepted,
    /// Not for this module: no key, or no agent to ask
    Ignored(String),
    /// Asked and not approved, or a bad answer
    Rejected(String),
}

/// The module's logic: ask the agent, and check its signature
#[cfg(feature = "verify")]
pub fn authenticate(env: &dyn ModuleEnv) -> Outcome {
    use ssh_key::{PublicKey, SshSig};

    let ctx = match env.context() {
        Ok(ctx) => ctx,
        Err(err) => return Outcome::Ignored(err),
    };
    let keys = match env.authorized_keys(&ctx.user) {
        Ok(Some(text)) => text,
        Ok(None) => return Outcome::Ignored(format!("{} has no key", ctx.user)),
        Err(err) => return Outcome::Ignored(err),
    };
    let keys: Vec<PublicKey> = keys
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| PublicKey::from_openssh(line).ok())
        .collect();
    if keys.is_empty() {
        return Outcome::Ignored(format!("{}'s key file lists no key", ctx.user));
    }
    let nonce = match env.nonce() {
        Ok(nonce) => nonce,
        Err(err) => return Outcome::Ignored(err),
    };
    let challenge = Challenge {
        nonce,
        host: ctx.host,
        user: ctx.user,
        target: ctx.target,
        cwd: ctx.cwd,
        argv: ctx.argv,
        session: ctx.session,
        cgroup: ctx.cgroup,
        issued_at: ctx.now,
        module_version: MODULE_VERSION,
    };
    let bytes = challenge.encode();
    let mut request = Vec::new();
    if let Err(err) = write_request(&mut request, &bytes) {
        return Outcome::Ignored(err.0);
    }
    let signature = match env.exchange(ctx.uid, &request) {
        Ok(Reply::Signed(pem)) => pem,
        Ok(Reply::Refused(reason)) => return Outcome::Rejected(reason),
        Err(err) => return Outcome::Ignored(err),
    };
    let Ok(signature) = SshSig::from_pem(signature.trim()) else {
        return Outcome::Rejected("the answer is not a signature".to_string());
    };
    if keys
        .iter()
        .any(|key| key.verify(NAMESPACE, &bytes, &signature).is_ok())
    {
        Outcome::Accepted
    } else {
        Outcome::Rejected("the signature is not by the user's key".to_string())
    }
}

/// Keys, for the agent
#[cfg(feature = "sign")]
pub mod keys {
    use super::{Error, NAMESPACE, error};
    use ssh_key::{Algorithm, HashAlg, LineEnding, PrivateKey, rand_core::OsRng};

    /// A new key, encrypted with `passphrase`: its OpenSSH private key file
    /// and its public key line
    pub fn generate(passphrase: &str, comment: &str) -> Result<(String, String), Error> {
        let mut key = PrivateKey::random(&mut OsRng, Algorithm::Ed25519)
            .map_err(|err| error(err.to_string()))?;
        key.set_comment(comment);
        let public = key
            .public_key()
            .to_openssh()
            .map_err(|err| error(err.to_string()))?;
        let encrypted = key
            .encrypt(&mut OsRng, passphrase)
            .map_err(|err| error(err.to_string()))?;
        let private = encrypted
            .to_openssh(LineEnding::LF)
            .map_err(|err| error(err.to_string()))?;
        Ok((private.to_string(), public))
    }

    /// The decrypted key, or an error if the passphrase is wrong
    pub fn unlock(private: &str, passphrase: &str) -> Result<PrivateKey, Error> {
        let key = PrivateKey::from_openssh(private).map_err(|err| error(err.to_string()))?;
        key.decrypt(passphrase)
            .map_err(|_| error("the password is not right"))
    }

    /// The public key line of a key file, without decrypting it, and so
    /// without its comment, which is encrypted with the key
    pub fn public(private: &str) -> Result<String, Error> {
        let key = PrivateKey::from_openssh(private).map_err(|err| error(err.to_string()))?;
        key.public_key()
            .to_openssh()
            .map_err(|err| error(err.to_string()))
    }

    /// An approval: the key's signature over a challenge, PEM-armoured
    pub fn sign(key: &PrivateKey, challenge: &[u8]) -> Result<String, Error> {
        let signature = key
            .sign(NAMESPACE, HashAlg::Sha512, challenge)
            .map_err(|err| error(err.to_string()))?;
        signature
            .to_pem(LineEnding::LF)
            .map_err(|err| error(err.to_string()))
    }
}

#[cfg(all(test, feature = "sign"))]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn challenge() -> Challenge {
        Challenge {
            nonce: [7; 32],
            host: "neo".into(),
            user: "twright".into(),
            target: "root".into(),
            cwd: "/home/twright/repo".into(),
            argv: vec!["sudo".into(), "-u".into(), "root".into(), "dnf".into(), "install".into(), "foo".into()],
            session: Some("ct-codex-repo-1a2b".into()),
            cgroup: "0::/user.slice/user-1000.slice".into(),
            issued_at: 1_790_000_000,
            module_version: MODULE_VERSION,
        }
    }

    #[test]
    fn challenges_round_trip_and_reject_damage() {
        let c = challenge();
        let bytes = c.encode();
        assert_eq!(Challenge::decode(&bytes).unwrap(), c);
        assert!(Challenge::decode(&bytes[..bytes.len() - 1]).is_err());
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(Challenge::decode(&longer).is_err());
        assert!(Challenge::decode(b"something else").is_err());
        let none = Challenge { session: None, ..c };
        assert_eq!(Challenge::decode(&none.encode()).unwrap().session, None);
    }

    #[test]
    fn the_command_leaves_out_sudo_and_its_options() {
        assert_eq!(challenge().command(), ["dnf", "install", "foo"]);
        let c = Challenge {
            argv: vec!["sudo".into(), "-n".into(), "--".into(), "-weird".into()],
            ..challenge()
        };
        assert_eq!(c.command(), ["-weird"]);
    }

    #[test]
    fn frames_round_trip() {
        let mut buf = Vec::new();
        write_request(&mut buf, b"abc").unwrap();
        assert_eq!(read_request(&mut buf.as_slice()).unwrap(), b"abc");
        for reply in [Reply::Signed("sig".into()), Reply::Refused("no".into())] {
            let mut buf = Vec::new();
            write_reply(&mut buf, &reply).unwrap();
            assert_eq!(read_reply(&mut buf.as_slice()).unwrap(), reply);
        }
    }

    /// A module's surroundings, with an agent that answers as told
    struct Fake {
        keys: Option<String>,
        answer: Box<dyn Fn(&[u8]) -> Result<Reply, String>>,
        seen: RefCell<Option<Challenge>>,
    }

    impl ModuleEnv for Fake {
        fn context(&self) -> Result<Context, String> {
            let c = challenge();
            Ok(Context {
                host: c.host,
                user: c.user,
                uid: 1000,
                target: c.target,
                cwd: c.cwd,
                argv: c.argv,
                session: c.session,
                cgroup: c.cgroup,
                now: c.issued_at,
            })
        }
        fn nonce(&self) -> Result<[u8; 32], String> {
            Ok([9; 32])
        }
        fn authorized_keys(&self, _user: &str) -> Result<Option<String>, String> {
            Ok(self.keys.clone())
        }
        fn exchange(&self, _uid: u32, request: &[u8]) -> Result<Reply, String> {
            let bytes = read_request(&mut &request[..]).map_err(|e| e.0)?;
            *self.seen.borrow_mut() = Some(Challenge::decode(&bytes).map_err(|e| e.0)?);
            (self.answer)(&bytes)
        }
    }

    fn fake(keys: Option<String>, answer: impl Fn(&[u8]) -> Result<Reply, String> + 'static) -> Fake {
        Fake {
            keys,
            answer: Box::new(answer),
            seen: RefCell::new(None),
        }
    }

    #[test]
    fn only_a_signature_by_the_enrolled_key_over_this_challenge_is_accepted() {
        let (private, public) = keys::generate("pw", "test").unwrap();
        let key = keys::unlock(&private, "pw").unwrap();
        let (other_private, _) = keys::generate("pw", "other").unwrap();
        let other = keys::unlock(&other_private, "pw").unwrap();
        assert!(keys::unlock(&private, "wrong").is_err());
        // The comment is inside the encryption, so it is not read back
        let key_data = |line: &str| line.split_whitespace().take(2).collect::<Vec<_>>().join(" ");
        assert_eq!(key_data(&keys::public(&private).unwrap()), key_data(&public));

        let good = fake(Some(format!("# comment\n{public}\n")), {
            let key = key.clone();
            move |bytes| Ok(Reply::Signed(keys::sign(&key, bytes).unwrap()))
        });
        assert_eq!(authenticate(&good), Outcome::Accepted);
        let seen = good.seen.borrow().clone().unwrap();
        assert_eq!(seen.nonce, [9; 32]);
        assert_eq!(seen.command(), ["dnf", "install", "foo"]);

        let wrong_key = fake(Some(public.clone()), move |bytes| {
            Ok(Reply::Signed(keys::sign(&other, bytes).unwrap()))
        });
        assert!(matches!(authenticate(&wrong_key), Outcome::Rejected(_)));

        // A signature over another challenge, as a replay would be
        let replay = fake(Some(public.clone()), {
            let key = key.clone();
            move |_| Ok(Reply::Signed(keys::sign(&key, &challenge().encode()).unwrap()))
        });
        assert!(matches!(authenticate(&replay), Outcome::Rejected(_)));

        let refused = fake(Some(public.clone()), |_| Ok(Reply::Refused("denied".into())));
        assert_eq!(authenticate(&refused), Outcome::Rejected("denied".into()));

        let garbage = fake(Some(public.clone()), |_| Ok(Reply::Signed("nonsense".into())));
        assert!(matches!(authenticate(&garbage), Outcome::Rejected(_)));

        let no_agent = fake(Some(public), |_| Err("no agent".into()));
        assert_eq!(authenticate(&no_agent), Outcome::Ignored("no agent".into()));

        let no_key = fake(None, |_| unreachable!());
        assert!(matches!(authenticate(&no_key), Outcome::Ignored(_)));
        let empty = fake(Some("# nothing\n".into()), |_| unreachable!());
        assert!(matches!(authenticate(&empty), Outcome::Ignored(_)));
    }
}
