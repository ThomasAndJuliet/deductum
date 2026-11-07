#!/usr/bin/env python3
"""Packaging provenance, immutable output and archive-boundary regressions."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import zipfile

spec = importlib.util.spec_from_file_location("package_windows", Path(__file__).with_name("package-windows.py"))
P = importlib.util.module_from_spec(spec)
spec.loader.exec_module(P)


class PackageWindows(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="Deductum packaging Å ")
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.root = self.base / "source"
        self.root.mkdir()
        self.runtime = self.base / "python.zip"
        with zipfile.ZipFile(self.runtime, "w") as archive:
            archive.writestr("python.exe", b"MZ-fixture-runtime")
            archive.writestr("python313.zip", b"fixture stdlib")
        self.digest = hashlib.sha256(self.runtime.read_bytes()).hexdigest()
        for folder in ("deductum/host/tests", "deductum/host/__pycache__", "deductum/backend/src", "packaging/windows"):
            (self.root / folder).mkdir(parents=True)
        files = {"Cargo.toml": '[package]\nversion = "0.1.0"\n', "LICENSE": "license",
                 "packaging/windows/README.txt": "portable package",
                 "deductum/host/deductum_host.py": "print('fixture')\n",
                 "deductum/host/drivers/isabelle_windows.sh": "#!/usr/bin/env bash\nprintf '%s' fixture\n",
                 "deductum/host/tests/test_secret.py": "must not be shipped",
                 "deductum/host/__pycache__/host.pyc": "must not be shipped",
                 "deductum/backend/src/backend.scala": "// fixture backend",
                 ".env": "FIXTURE_CREDENTIAL=not-a-real-secret", "state/session.json": "fixture private data",
                 ".ssh/id_fixture": "not-a-real-key"}
        for name, content in files.items():
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content, encoding="utf-8")
        (self.root / "packaging/windows/python-runtime.json").write_text(json.dumps({
            "version": "fixture", "url": "https://www.python.org/ftp/python/fixture/python.zip", "sha256": self.digest}), encoding="utf-8")
        self.git("init", "-q")
        self.git("add", ".")
        self.git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "fixture")
        self.sha = self.git("rev-parse", "HEAD").strip()
        self.binary = self.base / "deductum.exe"
        self.binary.write_bytes(b"MZ\0" + f"deductum-build:{self.sha}:clean".encode())
        self.output = self.base / "output"

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.root, text=True, stderr=subprocess.STDOUT)

    def package(self, **kwargs):
        return P.package(self.root, self.binary, self.runtime, self.output, **kwargs)

    def test_package_provenance_matches_exact_binary_runtime_and_host_bundle(self):
        archive, metadata = self.package()
        self.assertEqual(metadata["source_sha"], self.sha)
        self.assertFalse(metadata["source_dirty"])
        self.assertEqual(metadata["binary_sha256"], hashlib.sha256(self.binary.read_bytes()).hexdigest())
        self.assertEqual(metadata["archive_sha256"], hashlib.sha256(archive.read_bytes()).hexdigest())
        with zipfile.ZipFile(archive) as source:
            names = source.namelist()
            prefix = names[0].split("/")[0] + "/"
            release = json.loads(source.read(prefix + "share/deductum/RELEASE.json"))
            self.assertEqual(release["source_sha"], self.sha)
            self.assertEqual(release["python"]["sha256"], self.digest)
            self.assertEqual(source.read(prefix + "deductum.exe"), self.binary.read_bytes())
            self.assertIn(prefix + "python/python.exe", names)
            self.assertIn(prefix + "share/deductum/host/deductum_host.py", names)
            launcher = "deductum/host/drivers/isabelle_windows.sh"
            self.assertEqual(source.read(prefix + "share/" + launcher), (self.root / launcher).read_bytes())
            self.assertIn(prefix + "share/deductum/backend/src/backend.scala", names)
            for forbidden in ("/.git/", "/.ssh/", "/state/", "/tests/", "/__pycache__/", "/.env"):
                self.assertFalse(any(forbidden in name for name in names), "package included " + forbidden)

    def test_a_successful_package_is_immutable_and_repackaging_does_not_change_it(self):
        archive, _ = self.package()
        before = archive.read_bytes()
        with self.assertRaisesRegex(ValueError, "overwrite"):
            self.package()
        self.assertEqual(archive.read_bytes(), before)

    def test_old_revision_or_wrong_checkout_cleanliness_cannot_be_packaged(self):
        for stamp in ("deductum-build:" + "0" * 40 + ":clean", f"deductum-build:{self.sha}:dirty"):
            self.binary.write_bytes(b"MZ" + stamp.encode())
            with self.assertRaisesRegex(ValueError, "stamp"):
                self.package()
        self.assertFalse(list(self.output.glob("*.zip")) if self.output.exists() else [])

    def test_dirty_source_requires_explicit_development_opt_in_and_matching_binary(self):
        (self.root / "deductum/host/deductum_host.py").write_text("changed fixture\n")
        with self.assertRaisesRegex(ValueError, "clean checkout"):
            self.package()
        with self.assertRaisesRegex(ValueError, "stamp"):
            self.package(allow_dirty=True)
        self.binary.write_bytes(b"MZ" + f"deductum-build:{self.sha}:dirty".encode())
        archive, metadata = self.package(allow_dirty=True)
        self.assertTrue(metadata["source_dirty"])
        self.assertIn("-dev-", archive.name)

    def test_runtime_checksum_mismatch_cannot_publish_an_archive(self):
        self.runtime.write_bytes(self.runtime.read_bytes() + b"tampered")
        with self.assertRaisesRegex(ValueError, "checksum"):
            self.package()
        self.assertFalse(list(self.output.glob("*.zip")))

    def test_runtime_zip_cannot_write_outside_its_destination(self):
        for name in ("../escaped", "/absolute", "nested/../../escaped", "..\\escaped", "C:/escaped", "file:stream"):
            with self.subTest(name=name):
                archive = self.base / "unsafe.zip"
                with zipfile.ZipFile(archive, "w") as source:
                    source.writestr("python.exe", b"MZ")
                    source.writestr(name, b"untrusted")
                with self.assertRaisesRegex(ValueError, "Unsafe path"):
                    P.extract_runtime(archive, self.base / "extraction", hashlib.sha256(archive.read_bytes()).hexdigest())
                self.assertFalse((self.base / "escaped").exists())
                self.assertFalse((self.base / "extraction").exists(), "unsafe archive was partly extracted")

    def test_missing_runtime_executable_is_a_packaging_error(self):
        archive = self.base / "missing.zip"
        with zipfile.ZipFile(archive, "w") as source:
            source.writestr("stdlib.zip", b"fixture")
        with self.assertRaisesRegex(ValueError, "no python.exe"):
            P.extract_runtime(archive, self.base / "extraction", hashlib.sha256(archive.read_bytes()).hexdigest())

    def test_bundle_identity_changes_for_source_contents_and_paths_but_not_manifest(self):
        bundle = self.root / "deductum"
        original = P.bundle_digest(bundle)
        (bundle / "RELEASE.json").write_text("fixture release metadata")
        self.assertEqual(P.bundle_digest(bundle), original)
        source = bundle / "host/deductum_host.py"
        saved = source.read_bytes()
        source.write_bytes(saved + b"# source changed\n")
        self.assertNotEqual(P.bundle_digest(bundle), original)
        source.write_bytes(saved)
        source.rename(source.with_name("renamed_host.py"))
        self.assertNotEqual(P.bundle_digest(bundle), original)

    def test_runtime_download_requires_the_pinned_official_origin(self):
        with self.assertRaisesRegex(ValueError, "official python.org"):
            P.fetch_python(self.base / "download.zip", {"url": "https://example.invalid/python.zip", "sha256": self.digest})


if __name__ == "__main__":
    unittest.main()
