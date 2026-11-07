#!/usr/bin/env python3
"""Export the exact validated CI payload as a standalone, unsigned Flatpak bundle."""
import argparse
import configparser
import hashlib
import json
import os
from pathlib import Path
import runpy
import subprocess
import tarfile
import tempfile

APP_ID = "dev.twright.Deductum"
REPOSITORY = "ThomasAndJuliet/deductum"
RUNTIME = "org.freedesktop.Platform/x86_64/25.08"
ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def validate_source(directory, sha, run_id):
    receipt = json.loads((directory / "release.json").read_text())
    if not (
        receipt.get("repository") == REPOSITORY
        and receipt.get("sha") == sha
        and receipt.get("run_id") == run_id
        and receipt.get("app_id") == APP_ID
        and receipt.get("build_sha256") == digest(directory / "build.tar.gz")
    ):
        raise ValueError("Flatpak payload does not match the exact tested CI revision and digest")
    return receipt


def package(directory, sha, run_id):
    source = validate_source(directory, sha, run_id)
    output = directory / "linux"
    output.mkdir(exist_ok=True)
    bundle = output / f"deductum-{sha[:12]}-linux-x86_64.flatpak"
    if bundle.exists():
        raise ValueError("Refusing to overwrite a Flatpak bundle")
    with tempfile.TemporaryDirectory(prefix="deductum-bundle-") as temporary:
        root = Path(temporary)
        build = root / "build"
        build.mkdir()
        with tarfile.open(directory / "build.tar.gz") as archive:
            archive.extractall(build, filter="data")
        metadata = configparser.ConfigParser(interpolation=None)
        metadata.read(build / "metadata")
        app = metadata["Application"]
        if (app["name"], app["runtime"], app["command"]) != (APP_ID, RUNTIME, "deductum"):
            raise ValueError("Unexpected application, architecture, runtime or desktop command")
        runpy.run_path(str(ROOT / "scripts/check-flatpak-catalog"))["check"](build)
        subprocess.run(["desktop-file-validate", str(build / f"files/share/applications/{APP_ID}.desktop")], check=True)
        subprocess.run(["appstreamcli", "validate", "--no-net", str(build / f"files/share/metainfo/{APP_ID}.metainfo.xml")], check=True)
        subprocess.run(["flatpak", "build-export", str(root / "repo"), str(build), "stable"], check=True)
        subprocess.run(["flatpak", "build-bundle", "--arch=x86_64",
                        "--runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo",
                        str(root / "repo"), str(bundle.resolve()), APP_ID, "stable"], check=True)
    receipt = bundle.with_suffix(".json")
    receipt.write_text(json.dumps({
        "repository": REPOSITORY, "source_sha": sha, "source_dirty": False,
        "run_id": run_id, "validated_run_id": source.get("validated_run_id", run_id),
        "architecture": "x86_64", "app_id": APP_ID, "runtime": RUNTIME,
        "archive_sha256": digest(bundle), "archive_bytes": bundle.stat().st_size,
        "build_sha256": source["build_sha256"], "signed": False,
    }, sort_keys=True, indent=2) + "\n")
    return bundle, receipt


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    if os.environ.get("GITHUB_REPOSITORY") != REPOSITORY:
        parser.error("This packager is scoped to the private Deductum repository")
    print(package(args.directory, os.environ["GITHUB_SHA"], int(os.environ["GITHUB_RUN_ID"]))[0])
