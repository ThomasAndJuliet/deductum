#!/usr/bin/env python3
"""Preserve an exact, fully tested Windows CI package in a private GitHub Release."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile

REPOSITORY = "ThomasAndJuliet/deductum"


def validate_run(run, run_id):
    if not (
        run.get("id") == run_id
        and run.get("repository", {}).get("full_name") == REPOSITORY
        and run.get("repository", {}).get("private") is True
        and run.get("head_repository", {}).get("full_name") == REPOSITORY
        and run.get("path") == ".github/workflows/ci.yml"
        and run.get("event") == "push"
        and run.get("head_branch") == "main"
        and run.get("status") == "completed"
        and run.get("conclusion") == "success"
        and re.fullmatch(r"[0-9a-f]{40}", run.get("head_sha", ""))
    ):
        raise ValueError("Publication requires a successful private main-branch CI push from this repository")
    return run["head_sha"]


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def validate_package(directory, sha):
    archives = list(directory.glob("*.zip"))
    if len(archives) != 1:
        raise ValueError("Expected exactly one Windows portable ZIP")
    archive = archives[0]
    if not re.fullmatch(r"deductum-[\w.-]+-" + sha[:12] + r"-windows-x86_64\.zip", archive.name):
        raise ValueError("Unexpected Windows package name or source revision")
    receipt = archive.with_suffix(".json")
    metadata = json.loads(receipt.read_text(encoding="utf-8"))
    if not (
        metadata.get("repository") == REPOSITORY
        and metadata.get("source_sha") == sha
        and metadata.get("source_dirty") is False
        and metadata.get("architecture") == "x86_64"
        and metadata.get("archive_bytes") == archive.stat().st_size
        and metadata.get("archive_sha256") == digest(archive)
    ):
        raise ValueError("Windows package does not match its tested source/checksum receipt")
    return archive, receipt


def gh(*args):
    return subprocess.check_output(["gh", *map(str, args)], text=True)


def publish(run_id):
    run = json.loads(gh("api", f"repos/{REPOSITORY}/actions/runs/{run_id}"))
    sha = validate_run(run, run_id)
    with tempfile.TemporaryDirectory(prefix="deductum-release-") as temporary:
        root = Path(temporary)
        artifact = root / "artifact"
        gh("run", "download", run_id, "--repo", REPOSITORY,
           "--name", "windows-x86_64", "--dir", artifact)
        assets = validate_package(artifact, sha)
        tag = f"build-{sha}"
        # Query releases through the API: network/auth failures must not be
        # mistaken for a missing release. Paginate to cover all prior builds.
        rows = gh("api", "--paginate", f"repos/{REPOSITORY}/releases?per_page=100", "--jq", ".[] | @json")
        releases = (json.loads(row) for row in rows.splitlines())
        release = next((item for item in releases if item["tag_name"] == tag), None)
        if release is None:
            notes = root / "notes.md"
            notes.write_text(
                f"Windows x64 portable app, built and tested by [CI run {run_id}]"
                f"(https://github.com/{REPOSITORY}/actions/runs/{run_id}).\n\n"
                f"Source revision: `{sha}`. The JSON asset records the ZIP's SHA-256 checksum.\n\n"
                "Extract the app ZIP and run `deductum.exe`. Python is included; "
                "install Lean or Isabelle separately for local proofs. For updates, close the "
                "old app and run its `stop-host.cmd` before opening the new package.\n\n"
                "Linux: install the prebuilt signed Flatpak from the private `home-apps` remote "
                "with `flatpak install --user home-apps dev.twright.Deductum//stable`.\n",
                encoding="utf-8")
            gh("release", "create", tag, "--repo", REPOSITORY, "--target", sha,
               "--draft", "--title", f"Deductum build {sha[:12]}", "--notes-file", notes)
            release = {"draft": True, "target_commitish": sha, "assets": []}
        if release["target_commitish"] != sha:
            raise ValueError("Existing release targets a different source revision")
        names = {asset["name"] for asset in release["assets"]}
        for asset in assets:
            if asset.name in names:
                existing = root / "existing" / asset.name
                gh("release", "download", tag, "--repo", REPOSITORY,
                   "--pattern", asset.name, "--dir", existing.parent)
                if digest(existing) != digest(asset):
                    raise ValueError(f"Refusing to replace immutable release asset {asset.name}")
            elif release["draft"]:
                gh("release", "upload", tag, asset, "--repo", REPOSITORY)
            else:
                raise ValueError(f"Published release is missing {asset.name}")
        if release["draft"]:
            # Unique releases preserve every build. A delayed older run never
            # changes the latest pointer or overwrites another revision.
            gh("release", "edit", tag, "--repo", REPOSITORY, "--draft=false", "--latest=false")
        print(f"https://github.com/{REPOSITORY}/releases/tag/{tag}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-id", type=int, required=True)
    args = parser.parse_args()
    if args.run_id <= 0:
        parser.error("run ID must be positive")
    publish(args.run_id)
