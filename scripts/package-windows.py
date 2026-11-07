#!/usr/bin/env python3
"""Package a linked Windows Deductum binary with its pinned native Python host.

Release packages require a clean checkout and a binary stamped with that exact
commit. --allow-dirty creates an explicitly marked development artifact only.
No source credentials, user state, signing key or installed prover is bundled.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import tempfile
import tomllib
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def sha256(path: Path) -> str:
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def read_runtime(root: Path) -> dict:
    return json.loads((root / "packaging/windows/python-runtime.json").read_text(encoding="utf-8"))


def fetch_python(destination: Path, manifest: dict) -> Path:
    """Only the pinned official HTTPS payload may become the bundled runtime."""
    if not manifest["url"].startswith("https://www.python.org/ftp/python/"):
        raise ValueError("Python runtime must come from the pinned official python.org URL")
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.is_file() and sha256(destination) == manifest["sha256"]:
        return destination
    temporary = destination.with_name(destination.name + ".part")
    try:
        with urllib.request.urlopen(manifest["url"], timeout=120) as source, temporary.open("wb") as output:
            shutil.copyfileobj(source, output)
        if sha256(temporary) != manifest["sha256"]:
            raise ValueError("Downloaded embedded Python checksum does not match the pinned runtime")
        os.replace(temporary, destination)
    finally:
        temporary.unlink(missing_ok=True)
    return destination


def extract_runtime(archive: Path, destination: Path, expected_sha256: str) -> None:
    if sha256(archive) != expected_sha256:
        raise ValueError("Embedded Python checksum does not match the pinned runtime")
    with zipfile.ZipFile(archive) as source:
        for entry in source.infolist():
            name = PurePosixPath(entry.filename.replace("\\", "/"))
            if name.is_absolute() or ".." in name.parts or ":" in entry.filename:
                raise ValueError("Unsafe path in embedded Python archive")
        source.extractall(destination)
    if not (destination / "python.exe").is_file():
        raise ValueError("Embedded Python archive has no python.exe")


def bundle_digest(bundle: Path) -> str:
    digest = hashlib.sha256()
    for path in sorted(bundle.rglob("*")):
        if path.is_file() and path.name != "RELEASE.json":
            digest.update(path.relative_to(bundle).as_posix().encode("utf-8") + b"\0")
            digest.update(path.read_bytes())
            digest.update(b"\0")
    return digest.hexdigest()


def package(root: Path, binary: Path, python_zip: Path, output: Path, *, allow_dirty: bool = False,
            extra_dlls: tuple[Path, ...] = ()) -> tuple[Path, dict]:
    def git(*args: str) -> str:
        return subprocess.check_output(["git", *args], cwd=root, text=True).strip()

    source_sha = git("rev-parse", "HEAD")
    dirty = bool(git("status", "--porcelain", "--untracked-files=all"))
    if dirty and not allow_dirty:
        raise ValueError("Release packaging requires a clean checkout; use --allow-dirty only for development")
    raw = binary.read_bytes()
    if raw[:2] != b"MZ":
        raise ValueError("The application is not a Windows executable")
    expected_stamp = f"deductum-build:{source_sha}:{'dirty' if dirty else 'clean'}".encode()
    if expected_stamp not in raw:
        raise ValueError("Binary build stamp does not match this checkout; rebuild the exact revision before packaging")
    runtime = read_runtime(root)
    version = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    name = f"deductum-{version}-{source_sha[:12]}{'-dev' if dirty else ''}-windows-x86_64"
    output.mkdir(parents=True, exist_ok=True)
    archive = output / (name + ".zip")
    if archive.exists():
        raise ValueError(f"Refusing to overwrite existing package: {archive}")
    with tempfile.TemporaryDirectory(prefix="deductum-package-") as temporary:
        stage = Path(temporary) / name
        stage.mkdir()
        shutil.copy2(binary, stage / "deductum.exe")
        for dll in extra_dlls:
            if dll.suffix.lower() != ".dll" or (stage / dll.name).exists():
                raise ValueError("Extra runtime DLLs must have distinct .dll names")
            shutil.copy2(dll, stage / dll.name)
        extract_runtime(python_zip, stage / "python", runtime["sha256"])
        bundle = stage / "share/deductum"
        for folder in ("host", "backend"):
            shutil.copytree(root / "deductum" / folder, bundle / folder,
                            ignore=shutil.ignore_patterns("__pycache__", "*.pyc", "tests", "lib"))
        metadata = {
            "repository": "ThomasAndJuliet/deductum", "source_sha": source_sha,
            "source_dirty": dirty, "version": version, "architecture": "x86_64",
            "bundle_sha256": bundle_digest(bundle), "binary_sha256": sha256(binary),
            "python": runtime,
        }
        (bundle / "RELEASE.json").write_text(json.dumps(metadata, sort_keys=True, indent=2) + "\n", encoding="utf-8")
        (stage / "stop-host.cmd").write_text(
            '@echo off\n"%~dp0python\\python.exe" -X utf8 "%~dp0share\\deductum\\host\\deductum_host.py" stop-host\n',
            encoding="utf-8", newline="\r\n")
        shutil.copy2(root / "packaging/windows/README.txt", stage / "README.txt")
        shutil.copy2(root / "LICENSE", stage / "LICENSE")
        # Embedded fonts/icons keep their redistribution notices beside the app.
        for folder in (root / "res/fonts", root / "res/deductum/windows-fonts", root / "res/icons"):
            for notice in folder.glob("LICENSE*"):
                target = stage / "licenses" / folder.relative_to(root) / notice.name
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(notice, target)
        # A fixed timestamp and ordering keep equivalent inputs reproducible.
        with zipfile.ZipFile(archive, "x", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as target:
            for path in sorted(stage.rglob("*")):
                if path.is_file():
                    info = zipfile.ZipInfo(path.relative_to(Path(temporary)).as_posix(), date_time=(1980, 1, 1, 0, 0, 0))
                    info.compress_type = zipfile.ZIP_DEFLATED
                    info.external_attr = 0o100644 << 16
                    target.writestr(info, path.read_bytes())
    metadata["archive_sha256"] = sha256(archive)
    metadata["archive_bytes"] = archive.stat().st_size
    (output / (name + ".json")).write_text(json.dumps(metadata, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    return archive, metadata


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/x86_64-pc-windows-gnu/release/deductum.exe")
    parser.add_argument("--python-zip", type=Path, help="Already downloaded, checksum-verified official embedded Python ZIP")
    parser.add_argument("--output", type=Path, default=ROOT / "dist/windows")
    parser.add_argument("--allow-dirty", action="store_true")
    parser.add_argument("--dll", type=Path, action="append", default=[])
    args = parser.parse_args()
    runtime = args.python_zip or fetch_python(args.output / "downloads/python-embed-amd64.zip", read_runtime(ROOT))
    archive, metadata = package(ROOT, args.binary, runtime, args.output, allow_dirty=args.allow_dirty, extra_dlls=tuple(args.dll))
    print(json.dumps({"archive": str(archive), **metadata}, sort_keys=True))


if __name__ == "__main__":
    main()
