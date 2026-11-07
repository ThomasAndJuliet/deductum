#!/usr/bin/env python3
"""Publication must preserve only fully tested, private, immutable binaries."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("publisher", Path(__file__).with_name("publish-windows-release.py"))
publisher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publisher)
SHA = "a" * 40
RUN = {
    "id": 123, "repository": {"full_name": publisher.REPOSITORY, "private": True},
    "head_repository": {"full_name": publisher.REPOSITORY},
    "path": ".github/workflows/ci.yml", "event": "push", "head_branch": "main",
    "status": "completed", "conclusion": "success", "head_sha": SHA,
}


def package(directory):
    directory.mkdir(parents=True, exist_ok=True)
    archive = directory / f"deductum-1.9.0-{SHA[:12]}-windows-x86_64.zip"
    archive.write_bytes(b"immutable tested package")
    metadata = {
        "repository": publisher.REPOSITORY, "source_sha": SHA, "source_dirty": False,
        "architecture": "x86_64", "archive_bytes": archive.stat().st_size,
        "archive_sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
    }
    receipt = archive.with_suffix(".json")
    receipt.write_text(json.dumps(metadata), encoding="utf-8")
    return archive, receipt


class RunTests(unittest.TestCase):
    def test_accepts_only_completed_successful_private_main_ci_pushes(self):
        self.assertEqual(publisher.validate_run(RUN, 123), SHA)
        changes = [
            {"id": 124}, {"repository": {"full_name": "elsewhere/deductum", "private": True}},
            {"repository": {"full_name": publisher.REPOSITORY, "private": False}},
            {"head_repository": {"full_name": "attacker/deductum"}},
            {"path": ".github/workflows/other.yml"}, {"event": "pull_request"},
            {"head_branch": "feature"}, {"status": "in_progress"},
            {"conclusion": "failure"}, {"conclusion": "cancelled"},
            {"head_sha": "invalid"},
        ]
        for change in changes:
            with self.subTest(change=change), self.assertRaises(ValueError):
                publisher.validate_run({**RUN, **change}, 123)


class PackageTests(unittest.TestCase):
    def test_checks_exact_revision_clean_build_architecture_size_and_digest(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            assets = package(root)
            self.assertEqual(publisher.validate_package(root, SHA), assets)
            receipt = json.loads(assets[1].read_text())
            for field, value in [
                ("repository", "elsewhere/deductum"), ("source_sha", "b" * 40),
                ("source_dirty", True), ("source_dirty", None), ("architecture", "arm64"),
                ("archive_bytes", 0), ("archive_sha256", "0" * 64),
            ]:
                with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                    assets[1].write_text(json.dumps({**receipt, field: value}))
                    publisher.validate_package(root, SHA)
            assets[1].write_text(json.dumps(receipt))
            assets[0].write_bytes(b"tampered tested package")
            with self.assertRaises(ValueError):
                publisher.validate_package(root, SHA)

    def test_refuses_missing_ambiguous_or_wrongly_named_packages(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with self.assertRaises(ValueError):
                publisher.validate_package(root, SHA)
            archive, _ = package(root)
            extra = root / "other.zip"
            extra.write_bytes(b"ambiguous")
            with self.assertRaises(ValueError):
                publisher.validate_package(root, SHA)
            extra.unlink()
            archive.rename(extra)
            with self.assertRaises(ValueError):
                publisher.validate_package(root, SHA)


class PublicationTests(unittest.TestCase):
    def setUp(self):
        self.run = copy.deepcopy(RUN)
        self.release = None
        self.uploaded = {}
        self.calls = []

    def gh(self, *args):
        self.calls.append(args)
        if args[:2] == ("api", f"repos/{publisher.REPOSITORY}/actions/runs/123"):
            return json.dumps(self.run)
        if args[:2] == ("run", "download"):
            package(Path(args[args.index("--dir") + 1]))
        elif args[:1] == ("api",):
            return json.dumps(self.release) if self.release else ""
        elif args[:2] == ("release", "create"):
            self.release = {
                "tag_name": args[2], "target_commitish": args[args.index("--target") + 1],
                "draft": True, "assets": [],
            }
        elif args[:2] == ("release", "upload"):
            asset = Path(args[3])
            self.uploaded[asset.name] = asset.read_bytes()
            self.release["assets"].append({"name": asset.name})
        elif args[:2] == ("release", "download"):
            name = args[args.index("--pattern") + 1]
            directory = Path(args[args.index("--dir") + 1])
            directory.mkdir(parents=True, exist_ok=True)
            (directory / name).write_bytes(self.uploaded[name])
        elif args[:2] == ("release", "edit"):
            self.assertEqual(len(self.release["assets"]), 2, "publish only after both assets exist")
            self.assertIn("--latest=false", args)
            self.release["draft"] = False
        else:
            self.fail(f"Unexpected gh call: {args}")
        return ""

    def test_creates_complete_release_and_retries_without_replacing_assets(self):
        with patch.object(publisher, "gh", self.gh):
            publisher.publish(123)
            self.assertFalse(self.release["draft"])
            self.assertEqual(self.release["tag_name"], f"build-{SHA}")
            self.assertEqual(len(self.uploaded), 2)
            self.calls.clear()
            publisher.publish(123)
            self.assertFalse(any(call[:2] in [("release", "upload"), ("release", "edit"),
                                             ("release", "create")] for call in self.calls))

    def test_recovers_partially_uploaded_draft_without_overwriting_it(self):
        def interrupted(*args):
            if args[:2] == ("release", "upload") and len(self.uploaded) == 1:
                raise subprocess.CalledProcessError(1, ["gh", "release", "upload"])
            return self.gh(*args)

        with patch.object(publisher, "gh", interrupted), self.assertRaises(subprocess.CalledProcessError):
            publisher.publish(123)
        self.assertTrue(self.release["draft"])
        self.calls.clear()
        with patch.object(publisher, "gh", self.gh):
            publisher.publish(123)
        self.assertFalse(self.release["draft"])
        self.assertEqual(sum(call[:2] == ("release", "upload") for call in self.calls), 1)

    def test_refuses_to_change_a_published_package(self):
        with patch.object(publisher, "gh", self.gh):
            publisher.publish(123)
            name = next(iter(self.uploaded))
            self.uploaded[name] = b"changed release asset"
            with self.assertRaisesRegex(ValueError, "immutable"):
                publisher.publish(123)

    def test_never_downloads_or_publishes_from_failed_ci(self):
        self.run["conclusion"] = "failure"
        with patch.object(publisher, "gh", self.gh), self.assertRaises(ValueError):
            publisher.publish(123)
        self.assertEqual(len(self.calls), 1)


if __name__ == "__main__":
    unittest.main()
