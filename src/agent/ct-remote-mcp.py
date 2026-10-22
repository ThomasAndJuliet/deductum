#!/usr/bin/env python3
# ct-remote-mcp: run an MCP server that shows windows on the machine the
# user is at, which may not be this one.
#
# Installed by COSMIC Terminal on every machine it works with, at
# ~/.config/cosmic-term-agents/ct-remote-mcp, and registered with the agents
# in place of the server itself.
#
# Usage: ct-remote-mcp SERVER        (only "marginalium" for now)
#
# When the user is at this machine (`ct-mount --where` says "local"), the
# server simply runs here. Otherwise it runs on the user's machine over ssh,
# with stdio carried through: that machine mounts this one (`ct-mount
# --export`), the server starts in the mount of this directory and holds a
# lease on the mount while it runs, and file paths are translated both ways
# — this machine's /x in a request becomes the mount's …/x there, and the
# mount's paths in replies become this machine's again — so the agent here
# keeps seeing its own paths. The MCP stdio framing is one JSON object per
# line, which this relies on.
#
# Runs on every machine, so it uses only the standard library.

import json
import os
import shlex
import subprocess
import sys
import threading

CT_MOUNT = os.path.expanduser("~/.config/cosmic-term-agents/ct-mount")
STATE = os.path.join(
    os.environ.get("XDG_STATE_HOME") or os.path.expanduser("~/.local/state"),
    "cosmic-term",
    "mounts",
)

# How to start each server, here and on the user's machine, and which tool
# arguments are file paths
SERVERS = {
    "marginalium": {
        "start": 'if command -v marginalium >/dev/null; then set -- marginalium --mcp; '
        "else set -- flatpak run --command=marginalium io.github.twright.Marginalium --mcp; fi",
        "paths": ("path", "file"),
    },
}

# The desktop session's environment, which an ssh session lacks
DESKTOP_ENV = (
    "eval \"$(systemctl --user show-environment | grep -E "
    "'^(WAYLAND_DISPLAY|DISPLAY|DBUS_SESSION_BUS_ADDRESS|XDG_RUNTIME_DIR|XDG_CURRENT_DESKTOP)=' "
    "| sed 's/^/export /')\"\n"
)


def read(path):
    with open(path, encoding="utf-8") as f:
        return f.read().strip()


def run_here(server):
    script = 'export PATH="$HOME/.local/bin:$PATH"\n' + server["start"] + '\nexec "$@"\n'
    os.execvp("sh", ["sh", "-c", script])


def walk(value, change):
    """value with change applied to every string in it"""
    if isinstance(value, str):
        return change(value)
    if isinstance(value, list):
        return [walk(v, change) for v in value]
    if isinstance(value, dict):
        return {k: walk(v, change) for k, v in value.items()}
    return value


def remote_command(script):
    """script as one `sh -c '...'` argument that any login shell parses alike

    ssh hands the command to the remote login shell, which may be fish. Fish
    treats \\' and \\\\ as escapes inside single quotes, which POSIX shells do
    not, so ' becomes '"'"' and \\ becomes '"\\\\"': double-quoted, those mean
    the same everywhere. (ssh's stdin carries the MCP stream, so the script
    cannot be piped to `sh -s`.)"""
    quoted = "".join({"'": "'\"'\"'", "\\": "'\"\\\\\"'"}.get(c, c) for c in script)
    return f"sh -c '{quoted}'"


def main():
    name = sys.argv[1] if len(sys.argv) > 1 else ""
    server = SERVERS.get(name)
    if server is None:
        sys.exit(f"ct-remote-mcp: unknown server {name!r}; known: {', '.join(SERVERS)}")
    where = subprocess.run([CT_MOUNT, "--where"], capture_output=True, text=True).stdout.split()
    if not where or where[0] != "remote":
        run_here(server)
    client = where[1]
    # Machines know each other by their host names
    me = read("/proc/sys/kernel/hostname")
    try:
        dest = read(os.path.join(STATE, f"{client}.dest"))
    except OSError as err:
        sys.exit(f"ct-remote-mcp: {client} is not set up to reach this machine: {err}")
    exported = subprocess.run([CT_MOUNT, "--export", client], capture_output=True, text=True)
    if exported.returncode != 0:
        sys.exit(f"ct-remote-mcp: {client} cannot mount this machine: {exported.stderr.strip()}")
    mount = exported.stdout.strip().rstrip("/")
    here = os.getcwd()

    script = (
        DESKTOP_ENV
        + f"cd {shlex.quote(mount + here)} 2>/dev/null || cd {shlex.quote(mount)}\n"
        + 'export PATH="$HOME/.local/bin:$PATH"\n'
        + f'lease="$("$HOME/.config/cosmic-term-agents/ct-mount" --lease {shlex.quote(me)})/mcp-$$"\n'
        + server["start"]
        + '\nexec flock "$lease" "$@"\n'
    )
    remote = remote_command(script)
    proc = subprocess.Popen(
        ["ssh", "-T", "-o", "BatchMode=yes", "-o", "ServerAliveInterval=15", dest, remote],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
    )

    def to_there(path):
        path = os.path.abspath(os.path.join(here, os.path.expanduser(path)))
        return path if path.startswith(mount + "/") else mount + path

    def to_here(text):
        return text.replace(mount + "/", "/")

    def requests():
        for line in sys.stdin:
            line = line.strip()
            if not line:
                continue
            try:
                message = json.loads(line)
            except ValueError:
                message = None
            if isinstance(message, dict) and message.get("method") == "tools/call":
                arguments = (message.get("params") or {}).get("arguments")
                if isinstance(arguments, dict):
                    for key in server["paths"]:
                        if isinstance(arguments.get(key), str):
                            arguments[key] = to_there(arguments[key])
                line = json.dumps(message)
            proc.stdin.write((line + "\n").encode())
            proc.stdin.flush()
        proc.stdin.close()

    threading.Thread(target=requests, daemon=True).start()
    for raw in proc.stdout:
        line = raw.decode(errors="replace").strip()
        if not line:
            continue
        try:
            line = json.dumps(walk(json.loads(line), to_here))
        except ValueError:
            pass
        sys.stdout.write(line + "\n")
        sys.stdout.flush()
    sys.exit(proc.wait())


main()
