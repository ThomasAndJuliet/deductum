#!/usr/bin/env python3
"""Publish tested Linux/Windows packages as a Conventional Commit SemVer release.

`stage` validates a successful CI run and uploads its packages to a draft;
`finish` makes the draft visible. In a public repository the workflow attests
the staged files between the two, so a public release is never visible
unattested. Visibility is GitHub's report of the repository, and public
publication also needs the signing decision recorded in release-config.json.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time

import release_config
import release_policy
import release_scan

REPOSITORY = release_config.repository()
PUBLISHER_WORKFLOW = ".github/workflows/release-binaries.yml"
# Signing identities a human may record as release-config.json's public_signing.
GITHUB_ATTESTATION = "github-attestation"


def validate_run(run, run_id):
    if not (
        run.get("id") == run_id
        and run.get("repository", {}).get("full_name") == REPOSITORY
        and type(run.get("repository", {}).get("private")) is bool
        and run.get("head_repository", {}).get("full_name") == REPOSITORY
        and run.get("path") == ".github/workflows/ci.yml"
        and run.get("event") == "push"
        and run.get("head_branch") == "main"
        and run.get("status") == "completed"
        and run.get("conclusion") == "success"
        and re.fullmatch(r"[0-9a-f]{40}", run.get("head_sha", ""))
    ):
        raise ValueError("Publication requires a successful main-branch CI push from this repository")
    return run["head_sha"]


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def is_public(run):
    return run["repository"]["private"] is False


def signing_decision(run):
    """The signing identity for this run, or None when publishing must wait."""
    if not is_public(run):
        return None
    decision = release_config.load().get("public_signing")
    if decision not in (None, GITHUB_ATTESTATION):
        raise ValueError("release-config.json names an unknown public_signing identity")
    return decision


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


def release_notes(plan, tag_assets, run_id, sha, public):
    changes, withheld = release_scan.public_lines(plan["notes"] or "Resuming the release reserved for this source revision.")
    if withheld:
        changes += f"\n\n{withheld} household infrastructure changes are not listed."
    linux = tag_assets[2].name
    signing = (
        "The packages have no platform signature: the Flatpak bundle has no repository "
        "signature and Windows may warn about an unknown publisher. "
        "SHA256SUMS records the download digests."
    )
    if public:
        signing += (
            " To check that a download came from this repository's release workflow, run "
            f"`gh attestation verify FILE --repo {REPOSITORY} "
            f"--signer-workflow {REPOSITORY}/{PUBLISHER_WORKFLOW}`."
        )
    notes = (
        f"{changes}\n\n"
        f"Linux and Windows x64 packages, built and tested by [CI run {run_id}]"
        f"(https://github.com/{REPOSITORY}/actions/runs/{run_id}).\n\n"
        f"Source revision: `{sha}`. JSON receipts record the package SHA-256 checksums.\n\n"
        "Windows: extract the app ZIP and run `deductum.exe`. Python is included; "
        "install Lean or Isabelle separately for local proofs. For updates, close the "
        "old app and run its `stop-host.cmd` before opening the new package.\n\n"
        f"Linux: `flatpak install --user --bundle {linux}` then "
        "`flatpak run dev.twright.Deductum`. Flathub supplies the runtime.\n\n"
        f"{signing}\n"
    )
    found = release_scan.text_findings(notes)
    if found:
        raise ValueError("Release notes name household systems: " + ", ".join(found))
    return notes


def require_public_payload(windows_archive, linux_receipt, receipts):
    """A public release shows nothing identifying the household's network."""
    found = set(release_scan.scan_zip(windows_archive))
    scan = json.loads(linux_receipt.read_text(encoding="utf-8")).get("public_scan")
    if not isinstance(scan, list):
        raise ValueError("Linux receipt has no public payload scan; build it with the current packager")
    found.update(scan)
    for receipt in receipts:
        found.update(release_scan.findings(receipt.read_bytes()))
    if found:
        raise ValueError("Refusing to publish a public package naming household systems: " + ", ".join(sorted(found)))


def stage(run_id, root):
    """Upload a tested run's packages to a draft; returns what remains to finish."""
    run = json.loads(gh("api", f"repos/{REPOSITORY}/actions/runs/{run_id}"))
    sha = validate_run(run, run_id)
    public = is_public(run)
    if public and signing_decision(run) is None:
        print("::notice::No public release: public_signing in release-config.json is not decided. "
              "A later release includes these commits.")
        return None
    plan = release_plan(sha)
    if plan is None:
        print("No release-relevant Conventional Commits.")
        return None
    # Old CI runs that complete after newer main pushes are covered by the
    # newer release's commit range. Already reserved releases can finish.
    if not plan["reserved"] and not current_main(sha):
        print("A newer main push supersedes this versioned release candidate.")
        return None
    root = Path(root)
    artifact = root / "artifact"
    gh("run", "download", run_id, "--repo", REPOSITORY,
       "--name", "windows-x86_64", "--dir", artifact)
    assets = list(validate_package(artifact, sha))
    linux = root / "linux"
    gh("run", "download", run_id, "--repo", REPOSITORY,
       "--name", "linux-x86_64", "--dir", linux)
    assets.extend(validate_linux_package(linux, sha, run_id))
    if public:
        require_public_payload(assets[0], assets[3], (assets[1], assets[3]))
    # Reserve only after both tested artifacts have been verified.
    plan = reserve_version(sha, plan)
    if plan is None:
        print("A newer main push supersedes this versioned release candidate.")
        return None
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
    checksums = downloads / "SHA256SUMS"
    checksums.write_text("".join(f"{digest(asset)}  {asset.name}\n" for asset in assets), encoding="utf-8")
    assets.append(checksums)
    # Query releases through the API: network/auth failures must not be
    # mistaken for a missing release. Paginate to cover all prior builds.
    release = find_release(tag)
    if release is None:
        notes = root / "notes.md"
        notes.write_text(release_notes(plan, assets, run_id, sha, public), encoding="utf-8")
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
    if not release["draft"]:
        print(f"https://github.com/{REPOSITORY}/releases/tag/{tag}")
        return None
    return {"tag": tag, "attest": signing_decision(run) == GITHUB_ATTESTATION, "assets": assets}


def find_release(tag):
    rows = gh("api", "--paginate", f"repos/{REPOSITORY}/releases?per_page=100", "--jq", ".[] | @json")
    return next((item for item in map(json.loads, rows.splitlines()) if item["tag_name"] == tag), None)


def verify_attestation(asset, attempts=3, pause=10):
    for attempt in range(attempts):
        try:
            gh("attestation", "verify", asset, "--repo", REPOSITORY,
               "--signer-workflow", f"{REPOSITORY}/{PUBLISHER_WORKFLOW}")
            return
        except subprocess.CalledProcessError:
            if attempt == attempts - 1:
                raise
            time.sleep(pause)


def finish(run_id, tag, root):
    """Make a completely staged draft visible, attested when the repository is public."""
    run = json.loads(gh("api", f"repos/{REPOSITORY}/actions/runs/{run_id}"))
    sha = validate_run(run, run_id)
    release = find_release(tag)
    if release is None or not release["draft"] or release["target_commitish"] != sha:
        raise ValueError("Finishing requires this source revision's draft release")
    downloads = Path(root) / "downloads"
    assets = sorted(downloads.iterdir())
    if {asset.name for asset in assets} != {asset["name"] for asset in release["assets"]}:
        raise ValueError("Draft release does not hold exactly the staged files")
    if is_public(run):
        if signing_decision(run) != GITHUB_ATTESTATION:
            raise ValueError("A public release needs its recorded signing identity")
        for asset in assets:
            verify_attestation(asset)
    # Unique tags preserve each version. A delayed older run never
    # changes the latest pointer or overwrites another revision.
    gh("release", "edit", tag, "--repo", REPOSITORY, "--draft=false", "--latest=false")
    print(f"https://github.com/{REPOSITORY}/releases/tag/{tag}")


def write_outputs(staged):
    path = os.environ.get("GITHUB_OUTPUT")
    if not path:
        return
    with open(path, "a", encoding="utf-8") as output:
        output.write(f"tag={staged['tag'] if staged else ''}\n")
        output.write(f"attest={'true' if staged and staged['attest'] else 'false'}\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("stage", "finish"))
    parser.add_argument("--run-id", type=int, required=True)
    parser.add_argument("--root", type=Path, required=True, help="Empty directory for this run's staged files")
    parser.add_argument("--tag", help="The tag to finish, from stage's output")
    args = parser.parse_args()
    if args.run_id <= 0:
        parser.error("run ID must be positive")
    if args.command == "stage":
        write_outputs(stage(args.run_id, args.root))
    elif args.tag and re.fullmatch(r"v\d+\.\d+\.\d+", args.tag):
        finish(args.run_id, args.tag, args.root)
    else:
        parser.error("finish needs the version tag from stage")


if __name__ == "__main__":
    sys.exit(main())
