#!/usr/bin/env python3
"""Check provenance and exercise real standalone Flatpak installation and update."""
import gzip
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("bundler", Path(__file__).with_name("package-flatpak-bundle.py"))
bundler = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bundler)
SHA = "a" * 40
ROOT = Path(__file__).resolve().parents[1]


class BundleTests(unittest.TestCase):
    def fixture(self, root, text):
        build = root / "build"
        files = build / "files"
        (files / "bin").mkdir(parents=True)
        (files / "bin/deductum").write_text("#!/bin/sh\n" + text + "\n")
        (files / "bin/deductum").chmod(0o755)
        for source, destination in [
            ("res/deductum/dev.twright.Deductum.desktop", f"share/applications/{bundler.APP_ID}.desktop"),
            ("res/deductum/dev.twright.Deductum.svg", f"share/icons/hicolor/scalable/apps/{bundler.APP_ID}.svg"),
            ("packaging/flatpak/dev.twright.Deductum.metainfo.xml", f"share/metainfo/{bundler.APP_ID}.metainfo.xml"),
        ]:
            dest = files / destination
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / source, dest)
        catalog = files / f"share/app-info/xmls/{bundler.APP_ID}.xml.gz"
        catalog.parent.mkdir(parents=True)
        with gzip.open(catalog, "wb") as output:
            output.write((files / f"share/metainfo/{bundler.APP_ID}.metainfo.xml").read_bytes())
        (build / "metadata").write_text(
            f"[Application]\nname={bundler.APP_ID}\nruntime={bundler.RUNTIME}\ncommand=deductum\n")
        export = build / "export"
        shutil.copytree(files / "share/applications", export / "share/applications")
        shutil.copytree(files / "share/icons", export / "share/icons")
        artifact = root / "artifact"
        artifact.mkdir()
        with tarfile.open(artifact / "build.tar.gz", "w:gz") as archive:
            for name in ("metadata", "files", "export"):
                archive.add(build / name, arcname=name)
        receipt = {"repository": bundler.REPOSITORY, "sha": SHA, "run_id": 123,
                   "app_id": bundler.APP_ID, "build_sha256": bundler.digest(artifact / "build.tar.gz")}
        (artifact / "release.json").write_text(json.dumps(receipt))
        return artifact

    def test_rejects_wrong_source_run_identity_and_tampered_archive(self):
        with tempfile.TemporaryDirectory() as temporary:
            artifact = self.fixture(Path(temporary), "exit 0")
            self.assertEqual(bundler.validate_source(artifact, SHA, 123)["sha"], SHA)
            for sha, run_id in [("b" * 40, 123), (SHA, 124)]:
                with self.subTest(sha=sha, run_id=run_id), self.assertRaises(ValueError):
                    bundler.validate_source(artifact, sha, run_id)
            with (artifact / "build.tar.gz").open("ab") as source:
                source.write(b"tampered")
            with self.assertRaises(ValueError):
                bundler.validate_source(artifact, SHA, 123)

    def test_installs_and_updates_without_a_home_apps_remote(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            environment = dict(os.environ, FLATPAK_USER_DIR=str(root / "installation"),
                               XDG_DATA_HOME=str(root / "data"), XDG_CONFIG_HOME=str(root / "config"),
                               XDG_CACHE_HOME=str(root / "cache"))
            for revision in (1, 2):
                directory = root / str(revision)
                directory.mkdir()
                artifact = self.fixture(directory, f"echo revision-{revision}")
                archive, receipt = bundler.package(artifact, SHA, 123)
                self.assertEqual(json.loads(receipt.read_text())["archive_sha256"], bundler.digest(archive))
                # Isolated user installation. --no-deps keeps this test offline;
                # the user instructions install the real runtime from Flathub.
                subprocess.run(["flatpak", "install", "--user", "--noninteractive", "--no-deps",
                                "--or-update", "--bundle", str(archive)], env=environment, check=True)
                location = subprocess.check_output(["flatpak", "info", "--user", "--show-location", bundler.APP_ID],
                                                   env=environment, text=True).strip()
                self.assertIn(f"revision-{revision}", (Path(location) / "files/bin/deductum").read_text())
                remotes = subprocess.check_output(["flatpak", "remotes", "--user", "--columns=name"],
                                                 env=environment, text=True)
                self.assertNotIn("home-apps", remotes)


if __name__ == "__main__":
    unittest.main()
