// SPDX-License-Identifier: GPL-3.0-only

//! Run a command so that Ctrl+Z suspends it and Enter resumes it.
//!
//! Usage: `ct-supervise PROGRAM [ARGS...]`
//!
//! A session's pane has no interactive shell, so nothing would resume an
//! agent that stops itself on Ctrl+Z, and shells differ in whether `fg` works
//! in a script (dash's does, bash's does not). This does the job control
//! itself: the command runs in its own process group, which owns the terminal.
//! When it stops, the terminal comes back here for a prompt, then returns to
//! the command, which is continued. The exit status is the command's.
//!
//! It is linked statically, so that the terminal can copy the same file to
//! any host it opens sessions on.

use std::ffi::CString;
use std::io::{BufRead, Write};
use std::os::unix::ffi::OsStrExt;
use std::process::exit;

const TTY: libc::c_int = libc::STDIN_FILENO;

fn give_terminal(pgrp: libc::pid_t) {
    // SAFETY: a plain syscall; it fails harmlessly without a terminal
    unsafe {
        libc::tcsetpgrp(TTY, pgrp);
    }
}

fn main() {
    let args: Vec<CString> = std::env::args_os()
        .skip(1)
        .map(|arg| CString::new(arg.as_bytes()).expect("no NUL in arguments"))
        .collect();
    if args.is_empty() {
        eprintln!("usage: ct-supervise PROGRAM [ARGS...]");
        exit(2);
    }
    let mut argv: Vec<*const libc::c_char> = args.iter().map(|arg| arg.as_ptr()).collect();
    argv.push(std::ptr::null());

    // SAFETY: single-threaded until exec; the child only makes syscalls and
    // execs
    let pid = unsafe {
        // Taking the terminal back from the stopped command is a background
        // write to it
        libc::signal(libc::SIGTTOU, libc::SIG_IGN);
        match libc::fork() {
            -1 => {
                eprintln!("ct-supervise: fork: {}", std::io::Error::last_os_error());
                exit(1);
            }
            0 => {
                libc::setpgid(0, 0);
                give_terminal(libc::getpgrp());
                libc::signal(libc::SIGTTOU, libc::SIG_DFL);
                libc::execvp(argv[0], argv.as_ptr());
                let err = std::io::Error::last_os_error();
                eprintln!("{}: {err}", args[0].to_string_lossy());
                libc::_exit(127);
            }
            pid => pid,
        }
    };
    // Also here, in case the parent runs before the child
    // SAFETY: a plain syscall
    unsafe {
        libc::setpgid(pid, pid);
    }
    give_terminal(pid);

    loop {
        let mut status = 0;
        // SAFETY: waiting for our own child
        if unsafe { libc::waitpid(pid, &mut status, libc::WUNTRACED) } == -1 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            exit(1);
        }
        if libc::WIFSTOPPED(status) {
            // SAFETY: a plain syscall
            give_terminal(unsafe { libc::getpgrp() });
            let mut stdout = std::io::stdout();
            let _ = stdout.write_all(b"\r\n[Suspended. Press Enter to resume.]\r\n");
            let _ = stdout.flush();
            let mut line = String::new();
            let read = std::io::stdin().lock().read_line(&mut line);
            // SAFETY: signalling our own child's process group
            unsafe {
                if matches!(read, Ok(0) | Err(_)) {
                    // The terminal went away
                    libc::killpg(pid, libc::SIGHUP);
                } else {
                    give_terminal(pid);
                }
                libc::killpg(pid, libc::SIGCONT);
            }
        } else if libc::WIFEXITED(status) {
            exit(libc::WEXITSTATUS(status));
        } else if libc::WIFSIGNALED(status) {
            exit(128 + libc::WTERMSIG(status));
        }
    }
}
