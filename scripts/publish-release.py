#!/usr/bin/env python3
"""Publish tested Linux/Windows packages as a Conventional Commit SemVer release."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

import release_policy

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


def validate_linux_package(directory, sha, run_id):
    archives = list(directory.glob("*.flatpak"))
    if len(archives) != 1:
        raise ValueError("Expected exactly one Linux Flatpak bundle")
    archive = archives[0]
    if archive.name != f"deductum-{sha[:12]}-linux-x86_64.flatpak":
        raise ValueError("Unexpected Linux package name or source revision")
    receipt = archive.with_suffix(".json")
    metadata = json.loads(receipt.read_text(encoding="utf-8"))
    if not (
        metadata.get("repository") == REPOSITORY
        and metadata.get("source_sha") == sha
        and metadata.get("source_dirty") is False
        and metadata.get("run_id") == run_id
        and metadata.get("architecture") == "x86_64"
        and metadata.get("app_id") == "dev.twright.Deductum"
        and metadata.get("runtime") == "org.freedesktop.Platform/x86_64/25.08"
        and metadata.get("archive_bytes") == archive.stat().st_size
        and metadata.get("archive_sha256") == digest(archive)
    ):
        raise ValueError("Linux bundle does not match its tested source/checksum receipt")
    return archive, receipt


def gh(*args):
    return subprocess.check_output(["gh", *map(str, args)], text=True)


def release_plan(sha):
    rows = gh("api", "--paginate", f"repos/{REPOSITORY}/tags?per_page=100",
              "--jq", ".[] | [.name, .commit.sha] | @json")
    return release_policy.plan(sha, tags=[json.loads(row) for row in rows.splitlines()])


def current_main(sha):
    return json.loads(gh("api", f"repos/{REPOSITORY}/git/ref/heads/main"))["object"]["sha"] == sha


def reserve_version(sha, plan):
    for _ in range(5):
        if plan is None or plan["reserved"]:
            return plan
        if not current_main(sha):
            return None
        tag = plan["tag"]
        try:
            gh("api", "--method", "POST", f"repos/{REPOSITORY}/git/refs",
               "-f", f"ref=refs/tags/{tag}", "-f", f"sha={sha}")
            return plan
        except subprocess.CalledProcessError:
            existing = json.loads(gh("api", f"repos/{REPOSITORY}/git/ref/tags/{tag}"))
            if existing["object"]["sha"] == sha:
                return plan
            # Another tested ancestor won the reservation while these
            # artifacts downloaded. Recalculate from fresh API tags; no git
            # credentials or changes to an existing tag are needed.
            plan = release_plan(sha)
    raise ValueError("Version repeatedly reserved by another source; retry publication")


def publish(run_id):
    run = json.loads(gh("api", f"repos/{REPOSITORY}/actions/runs/{run_id}"))
    sha = validate_run(run, run_id)
    plan = release_plan(sha)
    if plan is None:
        print("No release-relevant Conventional Commits; signed Flatpak updates remain independent.")
        return
    # Old CI runs that complete after newer main pushes are covered by the
    # newer release's commit range. Already reserved releases can finish.
    if not plan["reserved"] and not current_main(sha):
        print("A newer main push supersedes this versioned release candidate.")
        return
    with tempfile.TemporaryDirectory(prefix="deductum-release-") as temporary:
        root = Path(temporary)
        artifact = root / "artifact"
        gh("run", "download", run_id, "--repo", REPOSITORY,
           "--name", "windows-x86_64", "--dir", artifact)
        assets = list(validate_package(artifact, sha))
        linux = root / "linux"
        gh("run", "download", run_id, "--repo", REPOSITORY,
           "--name", "linux-x86_64", "--dir", linux)
        assets.extend(validate_linux_package(linux, sha, run_id))
        # Reserve only after both tested artifacts have been verified.
        plan = reserve_version(sha, plan)
        if plan is None:
            print("A newer main push supersedes this versioned release candidate.")
            return
        tag = plan["tag"]
        # Give users versioned download names without modifying any tested
        # package or source receipt bytes.
        downloads = root / "downloads"
        downloads.mkdir()
        named_assets = []
        for platform, pair in (("windows-x86_64", assets[:2]), ("linux-x86_64", assets[2:])):
            for asset in pair:
                named = downloads / f"deductum-{tag}-{platform}{asset.suffix}"
                shutil.copyfile(asset, named)
                named_assets.append(named)
        assets = named_assets
        checksums = root / "SHA256SUMS"
        checksums.write_text("".join(f"{digest(asset)}  {asset.name}\n" for asset in assets), encoding="utf-8")
        assets.append(checksums)
        # Query releases through the API: network/auth failures must not be
        # mistaken for a missing release. Paginate to cover all prior builds.
        rows = gh("api", "--paginate", f"repos/{REPOSITORY}/releases?per_page=100", "--jq", ".[] | @json")
        releases = (json.loads(row) for row in rows.splitlines())
        release = next((item for item in releases if item["tag_name"] == tag), None)
        if release is None:
            notes = root / "notes.md"
            notes.write_text(
                (plan["notes"] or "Resuming the release reserved for this source revision.") + "\n\n"
                f"Linux and Windows x64 packages, built and tested by [CI run {run_id}]"
                f"(https://github.com/{REPOSITORY}/actions/runs/{run_id}).\n\n"
                f"Source revision: `{sha}`. JSON receipts record the package SHA-256 checksums.\n\n"
                "Extract the app ZIP and run `deductum.exe`. Python is included; "
                "install Lean or Isabelle separately for local proofs. For updates, close the "
                "old app and run its `stop-host.cmd` before opening the new package.\n\n"
                f"Linux: `flatpak install --user --bundle {assets[2].name}` then "
                "`flatpak run dev.twright.Deductum`. Flathub supplies the runtime. "
                "No home-apps remote is needed. The CI bundle is unsigned; "
                "SHA256SUMS records the download digests. Signed repository updates "
                "remain available through the private home-apps remote.\n",
                encoding="utf-8")
            gh("release", "create", tag, "--repo", REPOSITORY, "--target", sha,
               "--draft", "--title", f"Deductum {tag}", "--notes-file", notes)
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
            # Unique tags preserve each version. A delayed older run never
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
