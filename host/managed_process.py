"""Containment launcher: join the job before the prover can create any child."""
import os
import subprocess
import sys
from pathlib import Path

# Embedded CPython has an isolated module path that omits the script directory.
sys.path.insert(0, str(Path(__file__).resolve().parent))


def main():
    from _windows import join_job, watch_owner
    import msvcrt
    for fd in (0, 1, 2):
        msvcrt.setmode(fd, os.O_BINARY)
    handle = join_job(sys.argv[1])
    watch_owner(int(sys.argv[2]), int(sys.argv[3]), handle)
    # Keep our sole non-inheritable job handle until process exit. Explicitly closing
    # it would kill this launcher before Python could preserve the prover exit code.
    # Windows closes it on exit, terminating any surviving descendants as well.
    return subprocess.call(sys.argv[4:], stdin=sys.stdin.buffer, stdout=sys.stdout.buffer,
                           stderr=sys.stderr.buffer)


if __name__ == "__main__":
    sys.exit(main())
