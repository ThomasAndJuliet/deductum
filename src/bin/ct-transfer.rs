// SPDX-License-Identifier: GPL-3.0-only

//! Do one step of a session transfer on this computer.
//!
//! Usage: `ct-transfer < request.json > reply.json`
//!
//! Reads one request (see `cosmic_term::helper::Request`) as JSON on stdin
//! and writes one reply as JSON on stdout. The terminal runs it locally and
//! on the other computer over ssh, so it is linked statically, like
//! `ct-supervise`, and copied to each host named by its contents.

use std::io::{Read, Write};

use cosmic_term::{Env, helper};

fn main() {
    let mut input = String::new();
    let reply = match std::io::stdin().read_to_string(&mut input) {
        Ok(_) => match serde_json::from_str::<helper::Request>(&input) {
            Ok(request) => helper::answer(&Env::from_process(), request),
            Err(err) => helper::Reply::Error {
                message: format!("ct-transfer: bad request: {err}"),
            },
        },
        Err(err) => helper::Reply::Error {
            message: format!("ct-transfer: reading the request: {err}"),
        },
    };
    let mut stdout = std::io::stdout().lock();
    let written = serde_json::to_writer(&mut stdout, &reply)
        .map_err(std::io::Error::from)
        .and_then(|()| stdout.write_all(b"\n"));
    if written.is_err() {
        std::process::exit(1);
    }
}
