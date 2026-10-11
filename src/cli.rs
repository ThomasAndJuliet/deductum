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

use std::io::{BufRead, BufReader, Write};
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
        let abs = std::fs::canonicalize(&p).unwrap_or_else(|_| std::env::current_dir().unwrap_or_default().join(p));
        req.path = Some(abs.display().to_string());
    }
    (req != OpenRequest::default()).then_some(req)
}

fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    dir.join("deductum.sock")
}

/// Hand the request to a running window; true if one took it
pub fn forward(req: &OpenRequest) -> bool {
    let Ok(mut stream) = UnixStream::connect(socket_path()) else { return false };
    let Ok(line) = serde_json::to_string(req) else { return false };
    stream.write_all(format!("{line}\n").as_bytes()).is_ok()
}

/// Take requests from later `deductum` commands
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
        assert!(r.path.unwrap().starts_with('/'));
    }
}
