#!/usr/bin/env python3
"""Package linked macOS Deductum binaries (arm64 and x86_64) as one universal app
with a pinned Python runtime for each architecture.

Release packages require a clean checkout and binaries stamped with that exact
commit. --allow-dirty creates an explicitly marked development artifact only.
No source credentials, user state, signing key or installed prover is bundled.
The binary gets an ad-hoc signature only: it is not signed with a Developer ID.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import tarfile
import tempfile
import tomllib
import urllib.request
import zipfile

import release_config

ROOT = Path(__file__).resolve().parents[1]
ARCHES = ("arm64", "x86_64")
RUNTIME_ORIGIN = "https://github.com/astral-sh/python-build-standalone/releases/download/"
# Thin 64-bit Mach-O executables, as cargo writes them, in either byte order
MACHO_MAGICS = (b"\xcf\xfa\xed\xfe", b"\xfe\xed\xfa\xcf")


def sha256(path: Path) -> str:
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def read_runtime(root: Path) -> dict:
    return json.loads((root / "packaging/macos/python-runtime.json").read_text(encoding="utf-8"))


def fetch_python(destination: Path, manifest: dict) -> Path:
    """Only the pinned python-build-standalone HTTPS payload may become a bundled runtime."""
    if not manifest["url"].startswith(RUNTIME_ORIGIN):
        raise ValueError("Python runtime must come from the pinned python-build-standalone release URL")
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.is_file() and sha256(destination) == manifest["sha256"]:
        return destination
    temporary = destination.with_name(destination.name + ".part")
    try:
        with urllib.request.urlopen(manifest["url"], timeout=120) as source, temporary.open("wb") as output:
            shutil.copyfileobj(source, output)
        if sha256(temporary) != manifest["sha256"]:
            raise ValueError("Downloaded Python runtime checksum does not match the pinned runtime")
        os.replace(temporary, destination)
    finally:
        temporary.unlink(missing_ok=True)
    return destination


def extract_runtime(archive: Path, destination: Path, expected_sha256: str) -> None:
    """Extract an install_only archive, whose single top folder is `python`, to `destination`."""
    if sha256(archive) != expected_sha256:
        raise ValueError("Python runtime checksum does not match the pinned runtime")
    with tempfile.TemporaryDirectory(prefix="deductum-runtime-") as temporary:
        with tarfile.open(archive) as source:
            for entry in source.getmembers():
                name = PurePosixPath(entry.name)
                if name.is_absolute() or ".." in name.parts:
                    raise ValueError(f"Unsafe path in Python runtime archive: {entry.name}")
            # The data filter also refuses links that leave the tree and special files
            try:
                source.extractall(temporary, filter="data")
            except tarfile.FilterError as error:
                raise ValueError(f"Unsafe path in Python runtime archive: {error}") from error
        top = Path(temporary) / "python"
        if not (top / "bin/python3").is_file():
            raise ValueError("Python runtime archive has no python/bin/python3")
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.move(str(top), str(destination))


def bundle_digest(bundle: Path) -> str:
    digest = hashlib.sha256()
    for path in sorted(bundle.rglob("*")):
        if path.is_file() and path.name != "RELEASE.json":
            digest.update(path.relative_to(bundle).as_posix().encode("utf-8") + b"\0")
            digest.update(path.read_bytes())
            digest.update(b"\0")
    return digest.hexdigest()


def make_universal(binaries: dict[str, Path], output: Path) -> None:
    """Join the two thin binaries with lipo and give the result an ad-hoc signature,
    which Apple Silicon requires before it runs a binary that lipo has rewritten."""
    subprocess.run(["lipo", "-create", "-output", str(output), *(str(binaries[a]) for a in ARCHES)], check=True)
    subprocess.run(["codesign", "--force", "--sign", "-", str(output)], check=True)


def write_zip(stage: Path, archive: Path) -> None:
    """A fixed timestamp and ordering keep equivalent inputs reproducible. Unlike the Windows
    package, the permission bits and symbolic links are kept: the launcher and the runtime
    must stay executable, and the runtime links python3 to python3.12."""
    with zipfile.ZipFile(archive, "x", compression=zipfile.ZIP_DEFLATED, compresslevel=6) as target:
        for path in sorted(stage.rglob("*")):
            name = path.relative_to(stage.parent).as_posix()
            if path.is_symlink():
                info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
                info.create_system = 3
                info.external_attr = 0o120755 << 16
                target.writestr(info, os.readlink(path))
            elif path.is_file():
                info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
                info.create_system = 3
                info.compress_type = zipfile.ZIP_DEFLATED
                mode = 0o755 if os.access(path, os.X_OK) else 0o644
                info.external_attr = (0o100000 | mode) << 16
                target.writestr(info, path.read_bytes())


def package(root: Path, binaries: dict[str, Path], runtimes: dict[str, Path], output: Path, *,
            allow_dirty: bool = False, universal=make_universal) -> tuple[Path, dict]:
    def git(*args: str) -> str:
        return subprocess.check_output(["git", *args], cwd=root, text=True).strip()

    source_sha = git("rev-parse", "HEAD")
    dirty = bool(git("status", "--porcelain", "--untracked-files=all"))
    if dirty and not allow_dirty:
        raise ValueError("Release packaging requires a clean checkout; use --allow-dirty only for development")
    expected_stamp = f"deductum-build:{source_sha}:{'dirty' if dirty else 'clean'}".encode()
    for arch in ARCHES:
        raw = binaries[arch].read_bytes()
        if raw[:4] not in MACHO_MAGICS:
            raise ValueError(f"The {arch} application is not a thin macOS executable")
        if expected_stamp not in raw:
            raise ValueError(f"The {arch} binary build stamp does not match this checkout; "
                             "rebuild the exact revision before packaging")
    runtime = read_runtime(root)
    version = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    name = f"deductum-{version}-{source_sha[:12]}{'-dev' if dirty else ''}-macos-universal"
    output.mkdir(parents=True, exist_ok=True)
    archive = output / (name + ".zip")
    if archive.exists():
        raise ValueError(f"Refusing to overwrite existing package: {archive}")
    with tempfile.TemporaryDirectory(prefix="deductum-package-") as temporary:
        stage = Path(temporary) / name
        stage.mkdir()
        for arch in ARCHES:
            extract_runtime(runtimes[arch], stage / "python" / arch, runtime[arch]["sha256"])
        binary = stage / "deductum"
        universal(binaries, binary)
        binary.chmod(0o755)
        bundle = stage / "share/deductum"
        for folder in ("host", "backend"):
            shutil.copytree(root / folder, bundle / folder,
                            ignore=shutil.ignore_patterns("__pycache__", "*.pyc", "tests", "lib"))
        metadata = {
            "repository": release_config.repository(), "source_sha": source_sha,
            "source_dirty": dirty, "version": version,
            "architecture": "universal", "architectures": list(ARCHES),
            "bundle_sha256": bundle_digest(bundle), "binary_sha256": sha256(binary),
            "thin_binary_sha256": {arch: sha256(binaries[arch]) for arch in ARCHES},
            "python": {"version": runtime["version"], "release": runtime["release"],
                       **{arch: runtime[arch] for arch in ARCHES}},
        }
        (bundle / "RELEASE.json").write_text(json.dumps(metadata, sort_keys=True, indent=2) + "\n", encoding="utf-8")
        launcher = stage / "Deductum.command"
        shutil.copy2(root / "packaging/macos/Deductum.command", launcher)
        launcher.chmod(0o755)
        shutil.copy2(root / "packaging/macos/README.txt", stage / "README.txt")
        shutil.copy2(root / "LICENSE", stage / "LICENSE")
        # Embedded fonts and icons keep their redistribution notices beside the app
        for folder in (root / "res/fonts", root / "res/icons"):
            for notice in folder.glob("LICENSE*"):
                target = stage / "licenses" / folder.relative_to(root) / notice.name
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(notice, target)
        write_zip(stage, archive)
    metadata["archive_sha256"] = sha256(archive)
    metadata["archive_bytes"] = archive.stat().st_size
    (output / (name + ".json")).write_text(json.dumps(metadata, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    return archive, metadata


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary-arm64", type=Path, default=ROOT / "target/aarch64-apple-darwin/release/deductum")
    parser.add_argument("--binary-x86_64", type=Path, default=ROOT / "target/x86_64-apple-darwin/release/deductum")
    parser.add_argument("--output", type=Path, default=ROOT / "dist/macos")
    parser.add_argument("--allow-dirty", action="store_true")
    args = parser.parse_args()
    manifest = read_runtime(ROOT)
    runtimes = {arch: fetch_python(args.output / f"downloads/python-{arch}.tar.gz", manifest[arch]) for arch in ARCHES}
    binaries = {"arm64": args.binary_arm64, "x86_64": args.binary_x86_64}
    archive, metadata = package(ROOT, binaries, runtimes, args.output, allow_dirty=args.allow_dirty)
    print(json.dumps({"archive": str(archive), **metadata}, sort_keys=True))


if __name__ == "__main__":
    main()
