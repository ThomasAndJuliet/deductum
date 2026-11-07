"""Bounded real child processes for containment and binary-I/O tests."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time


def main():
    mode = sys.argv[1]
    if mode == "echo":
        if os.name == "nt":
            import msvcrt
            for fd in (0, 1, 2):
                msvcrt.setmode(fd, os.O_BINARY)
        sys.stdout.buffer.write(sys.stdin.buffer.read())
        sys.stderr.buffer.write(b"stderr\x00\xff\r\n")
        return 17
    root = Path(sys.argv[2])
    depth = int(sys.argv[3]) if len(sys.argv) > 3 else 0
    # Committed resident memory distinguishes counting a tree from its launcher.
    memory = bytearray(32 * 1024 * 1024) if depth == 1 else None
    if memory is not None:
        for i in range(0, len(memory), 4096):
            memory[i] = 1
    if depth < 2 and mode in ("tree", "orphan"):
        subprocess.Popen([sys.executable, __file__, "tree", str(root), str(depth + 1)],
                         stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                         stderr=subprocess.DEVNULL)
    (root / (str(depth) + ".json")).write_text(json.dumps({"pid": os.getpid()}))
    if mode == "orphan":
        deadline = time.monotonic() + 10
        while not (root / "2.json").exists() and time.monotonic() < deadline:
            time.sleep(.01)
        return 0
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline and not (root / "release").exists():
        if depth == 0 and (root / "exit-parent").exists():
            return 0
        time.sleep(.02)
    return 0


if __name__ == "__main__":
    sys.exit(main())
