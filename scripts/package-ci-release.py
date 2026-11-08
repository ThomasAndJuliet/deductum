#!/usr/bin/env python3
"""Validate a finished Deductum build and record the exact tested CI revision."""
import configparser
import hashlib
import json
import os
from pathlib import Path
import runpy
import subprocess
import sys

import release_config

APP_ID = "dev.twright.Deductum"
root = Path(__file__).resolve().parents[1]
if len(sys.argv) != 2:
    raise SystemExit("Usage: package-ci-release.py FINISHED_BUILD_DIR")
build = Path(sys.argv[1]).resolve()
metadata = configparser.ConfigParser(interpolation=None)
metadata.read(build / "metadata")
assert metadata["Application"]["name"] == APP_ID
assert metadata["Application"]["runtime"] == "org.freedesktop.Platform/x86_64/25.08"
assert metadata["Application"]["command"] == "deductum"
assert (build / "files/bin/deductum").is_file() and os.access(build / "files/bin/deductum", os.X_OK)
assert (build / "files/share/icons/hicolor/scalable/apps/dev.twright.Deductum.svg").is_file()
assert (build / "files/share/metainfo/dev.twright.Deductum.metainfo.xml").is_file()
runpy.run_path(str(root / "scripts/check-flatpak-catalog"))["check"](build)
subprocess.run(["desktop-file-validate", str(build / "files/share/applications/dev.twright.Deductum.desktop")], check=True)
subprocess.run(["appstreamcli", "validate", "--no-net", str(build / "files/share/metainfo/dev.twright.Deductum.metainfo.xml")], check=True)
# Trust only this checkout for these read-only provenance checks.
# SDK job-container ownership may differ from the hosted checkout owner.
git = ["git", "-c", "safe.directory=" + str(root)]
sha = subprocess.check_output([*git, "rev-parse", "HEAD"], cwd=root, text=True).strip()
assert os.environ["GITHUB_SHA"] == sha, "Build must package the checked-out CI revision"
subprocess.run([*git, "diff", "--quiet"], cwd=root, check=True)
subprocess.run([*git, "diff", "--cached", "--quiet"], cwd=root, check=True)
repository = os.environ["GITHUB_REPOSITORY"]
assert repository == release_config.repository()
run_id = int(os.environ["GITHUB_RUN_ID"])
assert run_id > 0
output = root / "ci-artifact"
output.mkdir(exist_ok=True)
archive = output / "build.tar.gz"
subprocess.run(["tar", "--hard-dereference", "--numeric-owner", "--owner=0", "--group=0", "-czf", str(archive), "-C", str(build), "metadata", "files", "export"], check=True)
digest = hashlib.file_digest(archive.open("rb"), "sha256").hexdigest()
(output / "release.json").write_text(json.dumps({"repository": repository, "sha": sha, "run_id": run_id, "app_id": APP_ID, "build_sha256": digest}, sort_keys=True) + "\n")
print(json.dumps({"sha": sha, "run_id": run_id, "build_sha256": digest, "archive_bytes": archive.stat().st_size}, sort_keys=True))
