#!/usr/bin/env python3
"""Build the disconnected GUI fixture without changing the production manifest.

Run heavy builds through the machine's build coordinator. This command only
builds a binary; run it inside an isolated VNC container with
DEDUCTUM_VNC_FIXTURE=1. It never opens a desktop window itself.
"""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    argparse.ArgumentParser(description=__doc__).parse_args()
    root = Path(__file__).resolve().parents[1]
    target = Path(os.environ.get("CARGO_TARGET_DIR", root / "target")).resolve()
    with tempfile.TemporaryDirectory(prefix="deductum-vnc-build-") as directory:
        work = Path(directory)
        for entry in root.iterdir():
            if entry.name.startswith(".") or entry.name in {
                "Cargo.toml", "target", "build-flatpak", "ci-artifact"
            }:
                continue
            (work / entry.name).symlink_to(entry, target_is_directory=entry.is_dir())
        # A distinct crate name keeps this main-thread launcher from replacing
        # libtest's normal deductum executable in a shared target directory.
        manifest = (root / "Cargo.toml").read_text() + '''
[[bin]]
name = "deductum-vnc"
path = "src/main.rs"
harness = false
'''
        (work / "Cargo.toml").write_text(manifest)
        subprocess.run([
            "cargo", "rustc", "--offline", "--locked", "--manifest-path",
            str(work / "Cargo.toml"), "--bin", "deductum-vnc", "--", "--cfg", "test",
        ], env={**os.environ, "CARGO_TARGET_DIR": str(target)}, check=True)
    print(target / "debug" / "deductum-vnc")


if __name__ == "__main__":
    main()
