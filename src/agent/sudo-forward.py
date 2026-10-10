#!/usr/bin/env python3
"""Prepare the remote public sudo socket for an SSH forward.

A local agent (or its systemd socket) is not a forward to this terminal.
Detach its public path without stopping its service or control socket.
An existing SSH listener remains usable. Detached Unix listeners cannot
receive new connections at the pathname, even if ss still lists them.
"""
import errno
import os
from pathlib import Path
import socket
import stat
import struct
import sys


def local_agent(peer):
    pid, uid, _ = peer
    if uid != os.getuid():
        raise RuntimeError("the sudo listener is not owned by this user")
    try:
        name = Path(os.readlink(f"/proc/{pid}/exe")).name
    except OSError:
        # A non-dumpable user systemd also hides exe, even from its own
        # user. comm remains readable and identifies the socket activator.
        try:
            name = Path(f"/proc/{pid}/comm").read_text().strip()
        except OSError:
            return False
    return name == "systemd" or name.startswith("ct-sudo-agent-") or name == "ct-sudo-agent"


def prepare(path):
    try:
        original = os.lstat(path)
    except FileNotFoundError:
        return path
    if not stat.S_ISSOCK(original.st_mode) or original.st_uid != os.getuid():
        raise RuntimeError("the sudo path is not this user's socket")
    with socket.socket(socket.AF_UNIX) as conn:
        conn.settimeout(2)
        try:
            conn.connect(path)
        except OSError as error:
            if error.errno not in (errno.ENOENT, errno.ECONNREFUSED):
                raise
        else:
            peer = struct.unpack("3i", conn.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
            if not local_agent(peer):
                return "live"
    try:
        current = os.lstat(path)
    except FileNotFoundError:
        return path
    if (current.st_dev, current.st_ino) != (original.st_dev, original.st_ino):
        raise RuntimeError("the sudo socket changed while checking it")
    os.unlink(path)
    return path


if __name__ == "__main__":
    print(prepare(sys.argv[1]))
