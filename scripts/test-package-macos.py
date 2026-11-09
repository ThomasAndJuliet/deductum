#!/usr/bin/env python3
"""macOS packaging provenance, immutable output, kept permissions and archive-boundary regressions.

lipo and codesign are replaced by a stand-in, so these tests run on Linux CI as well."""
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
import zipfile

import release_config

spec = importlib.util.spec_from_file_location("package_macos", Path(__file__).with_name("package-macos.py"))
P = importlib.util.module_from_spec(spec)
spec.loader.exec_module(P)

MACHO = b"\xcf\xfa\xed\xfe"


def runtime_archive(path: Path, extra: dict | None = None) -> str:
    """A fixture install_only archive: python/bin/python3.12, the python3 link and a stdlib file."""
    with tarfile.open(path, "w:gz") as archive:
        def add(name, data=b"", mode=0o644, link=None):
            info = tarfile.TarInfo(name)
            if link is not None:
                info.type, info.linkname = tarfile.SYMTYPE, link
                archive.addfile(info)
            else:
                info.size, info.mode = len(data), mode
                archive.addfile(info, io.BytesIO(data))
        add("python/bin/python3.12", MACHO + b"fixture-runtime", 0o755)
        add("python/bin/python3", link="python3.12")
        add("python/lib/python3.12/os.py", b"# fixture stdlib\n")
        for name, data in (extra or {}).items():
            add(name, data)
    return hashlib.sha256(path.read_bytes()).hexdigest()


def fake_universal(binaries, output):
    output.write_bytes(b"".join(binaries[arch].read_bytes() for arch in P.ARCHES))


class PackageMacos(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="Deductum packaging Å ")
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.root = self.base / "source"
        self.root.mkdir()
        self.runtimes = {arch: self.base / f"python-{arch}.tar.gz" for arch in P.ARCHES}
        self.digests = {arch: runtime_archive(path) for arch, path in self.runtimes.items()}
        files = {"Cargo.toml": '[package]\nversion = "0.1.0"\n', "LICENSE": "license",
                 "packaging/macos/README.txt": "macOS package",
                 "packaging/macos/Deductum.command": "#!/bin/sh\nexec \"$(dirname \"$0\")/deductum\" \"$@\"\n",
                 "res/fonts/LICENSE-fixture.txt": "font licence",
                 "host/deductum_host.py": "print('fixture')\n",
                 "host/tests/test_secret.py": "must not be shipped",
                 "host/__pycache__/host.pyc": "must not be shipped",
                 "backend/Isabelle2025/src/backend.scala": "// fixture backend",
                 ".env": "FIXTURE_CREDENTIAL=not-a-real-secret", "state/session.json": "fixture private data",
                 ".ssh/id_fixture": "not-a-real-key"}
        for name, content in files.items():
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content, encoding="utf-8")
        manifest = {"version": "fixture", "release": "fixture"}
        for arch in P.ARCHES:
            manifest[arch] = {"url": P.RUNTIME_ORIGIN + f"fixture/python-{arch}.tar.gz", "sha256": self.digests[arch]}
        (self.root / "packaging/macos/python-runtime.json").write_text(json.dumps(manifest), encoding="utf-8")
        self.git("init", "-q")
        self.git("add", ".")
        self.git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "fixture")
        self.sha = self.git("rev-parse", "HEAD").strip()
        self.binaries = {arch: self.base / f"deductum-{arch}" for arch in P.ARCHES}
        self.stamp("clean")
        self.output = self.base / "output"

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.root, text=True, stderr=subprocess.STDOUT)

    def stamp(self, state, sha=None):
        for arch, path in self.binaries.items():
            path.write_bytes(MACHO + f"{arch}\0deductum-build:{sha or self.sha}:{state}".encode())

    def package(self, **kwargs):
        return P.package(self.root, self.binaries, self.runtimes, self.output, universal=fake_universal, **kwargs)

    def test_package_provenance_matches_exact_binaries_runtimes_and_host_bundle(self):
        archive, metadata = self.package()
        self.assertEqual(metadata["source_sha"], self.sha)
        self.assertEqual(metadata["repository"], release_config.repository())
        self.assertFalse(metadata["source_dirty"])
        for arch in P.ARCHES:
            self.assertEqual(metadata["thin_binary_sha256"][arch], hashlib.sha256(self.binaries[arch].read_bytes()).hexdigest())
            self.assertEqual(metadata["python"][arch]["sha256"], self.digests[arch])
        self.assertEqual(metadata["archive_sha256"], hashlib.sha256(archive.read_bytes()).hexdigest())
        with zipfile.ZipFile(archive) as source:
            names = source.namelist()
            prefix = names[0].split("/")[0] + "/"
            release = json.loads(source.read(prefix + "share/deductum/RELEASE.json"))
            self.assertEqual(release["source_sha"], self.sha)
            self.assertEqual(source.read(prefix + "deductum"),
                             b"".join(self.binaries[a].read_bytes() for a in P.ARCHES))
            for arch in P.ARCHES:
                self.assertIn(prefix + f"python/{arch}/bin/python3.12", names)
            self.assertIn(prefix + "share/deductum/host/deductum_host.py", names)
            self.assertIn(prefix + "share/deductum/backend/Isabelle2025/src/backend.scala", names)
            self.assertIn(prefix + "licenses/res/fonts/LICENSE-fixture.txt", names)
            for forbidden in ("/.git/", "/.ssh/", "/state/", "/tests/", "/__pycache__/", "/.env"):
                self.assertFalse(any(forbidden in name for name in names), "package included " + forbidden)

    def test_launcher_and_runtime_stay_executable_and_the_runtime_link_survives(self):
        archive, _ = self.package()
        with zipfile.ZipFile(archive) as source:
            infos = {info.filename.split("/", 1)[1]: info for info in source.infolist()}
            for name in ("Deductum.command", "deductum", "python/arm64/bin/python3.12"):
                self.assertTrue((infos[name].external_attr >> 16) & 0o111, name + " lost its executable bit")
            self.assertFalse((infos["README.txt"].external_attr >> 16) & 0o111)
            link = infos["python/x86_64/bin/python3"]
            self.assertEqual((link.external_attr >> 16) & 0o170000, 0o120000)
            self.assertEqual(source.read(link), b"python3.12")

    def test_a_successful_package_is_immutable_and_repackaging_does_not_change_it(self):
        archive, _ = self.package()
        before = archive.read_bytes()
        with self.assertRaisesRegex(ValueError, "overwrite"):
            self.package()
        self.assertEqual(archive.read_bytes(), before)

    def test_old_revision_or_wrong_checkout_cleanliness_cannot_be_packaged(self):
        for state, sha in (("clean", "0" * 40), ("dirty", None)):
            self.stamp(state, sha)
            with self.assertRaisesRegex(ValueError, "stamp"):
                self.package()
        self.assertFalse(list(self.output.glob("*.zip")) if self.output.exists() else [])

    def test_one_stale_architecture_is_enough_to_refuse(self):
        self.binaries["x86_64"].write_bytes(MACHO + b"deductum-build:" + b"0" * 40 + b":clean")
        with self.assertRaisesRegex(ValueError, "x86_64 binary build stamp"):
            self.package()

    def test_a_binary_that_is_not_a_thin_mach_o_is_refused(self):
        self.binaries["arm64"].write_bytes(b"MZ" + f"deductum-build:{self.sha}:clean".encode())
        with self.assertRaisesRegex(ValueError, "not a thin macOS executable"):
            self.package()

    def test_dirty_source_requires_explicit_development_opt_in_and_matching_binaries(self):
        (self.root / "host/deductum_host.py").write_text("changed fixture\n")
        with self.assertRaisesRegex(ValueError, "clean checkout"):
            self.package()
        with self.assertRaisesRegex(ValueError, "stamp"):
            self.package(allow_dirty=True)
        self.stamp("dirty")
        archive, metadata = self.package(allow_dirty=True)
        self.assertTrue(metadata["source_dirty"])
        self.assertIn("-dev-", archive.name)

    def test_runtime_checksum_mismatch_cannot_publish_an_archive(self):
        path = self.runtimes["arm64"]
        path.write_bytes(path.read_bytes() + b"tampered")
        with self.assertRaisesRegex(ValueError, "checksum"):
            self.package()
        self.assertFalse(list(self.output.glob("*.zip")))

    def test_runtime_archive_cannot_write_outside_its_destination(self):
        for name in ("../escaped", "/absolute", "python/../../escaped"):
            with self.subTest(name=name):
                archive = self.base / "unsafe.tar.gz"
                digest = runtime_archive(archive, {name: b"untrusted"})
                with self.assertRaisesRegex(ValueError, "Unsafe path"):
                    P.extract_runtime(archive, self.base / "extraction", digest)
                self.assertFalse((self.base / "escaped").exists())
                self.assertFalse((self.base / "extraction").exists(), "unsafe archive was partly extracted")

    def test_missing_runtime_interpreter_is_a_packaging_error(self):
        archive = self.base / "missing.tar.gz"
        with tarfile.open(archive, "w:gz") as source:
            info = tarfile.TarInfo("python/lib/os.py")
            source.addfile(info, io.BytesIO(b""))
        with self.assertRaisesRegex(ValueError, "no python/bin/python3"):
            P.extract_runtime(archive, self.base / "extraction", hashlib.sha256(archive.read_bytes()).hexdigest())

    def test_runtime_download_requires_the_pinned_origin(self):
        with self.assertRaisesRegex(ValueError, "python-build-standalone"):
            P.fetch_python(self.base / "download.tar.gz", {"url": "https://example.invalid/python.tar.gz", "sha256": "0"})

    def test_the_pinned_runtime_manifest_names_both_architectures_from_the_pinned_origin(self):
        manifest = P.read_runtime(P.ROOT)
        for arch in P.ARCHES:
            self.assertTrue(manifest[arch]["url"].startswith(P.RUNTIME_ORIGIN))
            self.assertIn({"arm64": "aarch64", "x86_64": "x86_64"}[arch] + "-apple-darwin", manifest[arch]["url"])
            self.assertRegex(manifest[arch]["sha256"], "^[0-9a-f]{64}$")


if __name__ == "__main__":
    unittest.main()
