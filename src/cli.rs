// SPDX-License-Identifier: GPL-3.0-only

//! The `deductum` command (design C3), like `zed .`:
//!
//! ```text
//! deductum .
//! deductum /path/to/theories
//! deductum --machine d59500 /remote/path/to/theories
//! deductum --session SESSION_ID
//! ```
//!
//! A running window gets the request over a Unix socket and opens or focuses
//! the workspace; otherwise this process becomes the window. Local relative
//! paths are resolved here; remote ones are the machine's own paths. Paths are
//! passed as data, never through a shell.

#[cfg(unix)]
use std::io::{BufRead, BufReader};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use cosmic::app::Task;
use cosmic::iced::futures::SinkExt;
use cosmic::iced::futures::channel::mpsc::Sender;
use serde::{Deserialize, Serialize};

use super::Message;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OpenRequest {
    pub machine: Option<String>,
    pub path: Option<String>,
    pub session: Option<String>,
}

pub fn usage() -> &'static str {
    "usage: deductum [--machine NAME] [PATH | --session ID]"
}

pub fn parse(args: Vec<String>) -> Option<OpenRequest> {
    let mut req = OpenRequest::default();
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--machine" | "-m" => req.machine = it.next(),
            "--session" | "-s" => req.session = it.next(),
            "--help" | "-h" => {
                println!("{}", usage());
                std::process::exit(0);
            }
            "--build-info" => {
                println!("{}", serde_json::json!({
                    "source_sha": env!("DEDUCTUM_BUILD_SHA"),
                    "source_dirty": env!("DEDUCTUM_BUILD_STATE") != "clean",
                    "stamp": concat!("deductum-build:", env!("DEDUCTUM_BUILD_SHA"), ":", env!("DEDUCTUM_BUILD_STATE")),
                }));
                std::process::exit(0);
            }
            // The version stamp of the host sources this app carries (what it writes to an
            // installed host's VERSION), so that a deployment can install the host itself and the
            // app will not install it again, restarting it, when it next connects
            "--bundle-version" => {
                match super::link::bundle() {
                    Some(bundle) => println!("{}", super::link::bundle_version(&bundle)),
                    None => eprintln!("no host sources found beside this app"),
                }
                std::process::exit(0);
            }
            _ if arg.starts_with("--machine=") => req.machine = Some(arg["--machine=".len()..].to_string()),
            _ if arg.starts_with("--session=") => req.session = Some(arg["--session=".len()..].to_string()),
            _ => req.path = Some(arg),
        }
    }
    if req.machine.as_deref() == Some("local") {
        req.machine = None;
    }
    if req.machine.is_none()
        && let Some(path) = &req.path
    {
        let p = PathBuf::from(path);
        let abs = super::platform::canonicalize(&p).unwrap_or_else(|_| std::env::current_dir().unwrap_or_default().join(p));
        req.path = Some(abs.display().to_string());
    }
    (req != OpenRequest::default()).then_some(req)
}

#[cfg(unix)]
fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    dir.join("deductum.sock")
}

/// Hand the request to a running window; true if one took it
#[cfg(unix)]
pub fn forward(req: &OpenRequest) -> bool {
    let Ok(mut stream) = UnixStream::connect(socket_path()) else { return false };
    let Ok(line) = serde_json::to_string(req) else { return false };
    stream.write_all(format!("{line}\n").as_bytes()).is_ok()
}

/// Take requests from later `deductum` commands
#[cfg(unix)]
pub fn listen() -> Task<Message> {
    cosmic::task::stream(cosmic::iced::stream::channel(8, |mut output: Sender<Message>| async move {
        let path = socket_path();
        if UnixStream::connect(&path).is_err() {
            let _ = std::fs::remove_file(&path);
        }
        let Ok(listener) = tokio::net::UnixListener::bind(&path) else { return };
        while let Ok((stream, _)) = listener.accept().await {
            let std_stream = match stream.into_std() {
                Ok(s) => s,
                Err(_) => continue,
            };
            let _ = std_stream.set_nonblocking(false);
            let line = tokio::task::spawn_blocking(move || {
                let mut line = String::new();
                let _ = BufReader::new(std_stream).read_line(&mut line);
                line
            })
            .await
            .unwrap_or_default();
            if let Ok(req) = serde_json::from_str::<OpenRequest>(line.trim()) {
                let _ = output.send(Message::Open(req)).await;
            }
        }
    }))
}

#[cfg(windows)]
fn pipe_path(sid: &str) -> String {
    format!(r"\\.\pipe\deductum-{sid}")
}

#[cfg(windows)]
pub fn forward(req: &OpenRequest) -> bool {
    let Ok(sid) = super::platform::windows::current_sid() else { return false };
    forward_pipe(&pipe_path(&sid), &sid, req)
}

#[cfg(windows)]
fn forward_pipe(path: &str, sid: &str, req: &OpenRequest) -> bool {
    use std::os::windows::io::AsRawHandle;
    let Ok(mut pipe) = std::fs::OpenOptions::new().read(true).write(true).open(path) else { return false };
    if !matches!(super::platform::windows::pipe_has_owner(pipe.as_raw_handle().cast(), &sid), Ok(true)) {
        return false;
    }
    let Ok(line) = serde_json::to_string(req) else { return false };
    pipe.write_all(format!("{line}\n").as_bytes()).is_ok()
}

#[cfg(windows)]
fn make_pipe(sid: &str, path: &str, first: bool) -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    use tokio::net::windows::named_pipe::ServerOptions;
    let security = super::platform::windows::PipeSecurity::for_user(sid)?;
    let mut attributes = security.attributes();
    // CreateNamedPipe copies this descriptor. No security pointers survive the call.
    unsafe {
        ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(
                path,
                (&mut attributes as *mut windows_sys::Win32::Security::SECURITY_ATTRIBUTES).cast(),
            )
    }
}

#[cfg(windows)]
pub fn listen() -> Task<Message> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt};
    cosmic::task::stream(cosmic::iced::stream::channel(8, |mut output: Sender<Message>| async move {
        let sid = match super::platform::windows::current_sid() {
            Ok(sid) => sid,
            Err(error) => { log::error!("could not identify the local user for launch requests: {error}"); return; }
        };
        let path = pipe_path(&sid);
        let mut server = match make_pipe(&sid, &path, true) {
            Ok(server) => server,
            Err(error) => { log::error!("could not secure the launch-request pipe: {error}"); return; }
        };
        loop {
            if let Err(error) = server.connect().await {
                log::error!("launch-request connection failed: {error}");
                return;
            }
            let next = match make_pipe(&sid, &path, false) {
                Ok(next) => next,
                Err(error) => { log::error!("could not create the next launch-request pipe: {error}"); return; }
            };
            let connected = std::mem::replace(&mut server, next);
            let mut reader = tokio::io::BufReader::new(connected.take(1 << 20));
            let mut line = String::new();
            if matches!(tokio::time::timeout(std::time::Duration::from_secs(5), reader.read_line(&mut line)).await, Ok(Ok(_)))
                && line.ends_with('\n')
                && let Ok(req) = serde_json::from_str::<OpenRequest>(line.trim())
                && output.send(Message::Open(req)).await.is_err()
            {
                return;
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_requests() {
        let r = parse(vec!["--machine".into(), "d59500".into(), "/srv/UTP".into()]).unwrap();
        assert_eq!(r.machine.as_deref(), Some("d59500"));
        assert_eq!(r.path.as_deref(), Some("/srv/UTP"));
        let r = parse(vec!["--session".into(), "abc".into()]).unwrap();
        assert_eq!(r.session.as_deref(), Some("abc"));
        assert!(parse(vec![]).is_none());
        let r = parse(vec![".".into()]).unwrap();
        assert!(std::path::Path::new(&r.path.unwrap()).is_absolute());
    }

    #[test]
    fn remote_paths_are_passed_to_the_host_without_local_canonicalization() {
        let path = "/srv/proofs/with spaces/α/../lemma.lean";
        let request = parse(vec!["--machine".into(), "proof-server".into(), path.into()]).unwrap();
        assert_eq!(request.path.as_deref(), Some(path));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn a_launch_request_reaches_a_pipe_owned_by_this_user() {
        use tokio::io::AsyncReadExt;
        let sid = super::super::platform::windows::current_sid().unwrap();
        let path = format!(r"\\.\pipe\deductum-test-{}-allowed", std::process::id());
        let mut server = make_pipe(&sid, &path, true).expect("secure per-user named pipe");
        let request = OpenRequest { machine: Some("proof-server".into()), path: Some("/srv/α proof.lean".into()), session: None };
        let outgoing = request.clone();
        let sending = async { tokio::task::spawn_blocking(move || forward_pipe(&path, &sid, &outgoing)).await.unwrap() };
        let (connected, sent) = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            tokio::join!(server.connect(), sending)
        }).await.expect("pipe forwarding must complete within three seconds");
        connected.unwrap();
        assert!(sent, "same-user forwarding must authenticate the real server");
        let mut received = Vec::new();
        tokio::time::timeout(std::time::Duration::from_secs(3), server.read_to_end(&mut received)).await.unwrap().unwrap();
        assert_eq!(serde_json::from_slice::<OpenRequest>(&received).unwrap(), request);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn a_different_pipe_owner_receives_no_launch_request() {
        use tokio::io::AsyncReadExt;
        let sid = super::super::platform::windows::current_sid().unwrap();
        assert_ne!(sid, "S-1-5-18", "this acceptance suite must run as an ordinary user, not SYSTEM");
        let path = format!(r"\\.\pipe\deductum-test-{}-denied", std::process::id());
        let mut server = make_pipe(&sid, &path, true).unwrap();
        let request = OpenRequest { machine: None, path: Some("private-proof.lean".into()), session: None };
        let sending = async { tokio::task::spawn_blocking(move || forward_pipe(&path, "S-1-5-18", &request)).await.unwrap() };
        let (connected, sent) = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            tokio::join!(server.connect(), sending)
        }).await.expect("pipe forwarding must complete within three seconds");
        connected.unwrap();
        assert!(!sent, "a reachable pipe with the wrong owner is not a launch receiver");
        let mut received = Vec::new();
        tokio::time::timeout(std::time::Duration::from_secs(3), server.read_to_end(&mut received)).await.unwrap().unwrap();
        assert!(received.is_empty(), "owner rejection must happen before any request bytes are written");
    }
}
