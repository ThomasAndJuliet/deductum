#!/usr/bin/env python3
"""Publication must preserve only fully tested, immutable binaries, and public ones must be attested."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import zipfile

spec = importlib.util.spec_from_file_location("publisher", Path(__file__).with_name("publish-release.py"))
publisher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publisher)
SHA = "a" * 40
RUN = {
    "id": 123, "repository": {"full_name": publisher.REPOSITORY, "private": True},
    "head_repository": {"full_name": publisher.REPOSITORY},
    "path": ".github/workflows/ci.yml", "event": "push", "head_branch": "main",
    "status": "completed", "conclusion": "success", "head_sha": SHA,
}


def package(directory, windows_binary=b"immutable tested package"):
    directory.mkdir(parents=True, exist_ok=True)
    archive = directory / f"deductum-1.9.0-{SHA[:12]}-windows-x86_64.zip"
    with zipfile.ZipFile(archive, "w") as contents:
        contents.writestr("deductum/deductum.exe", windows_binary)
    metadata = {
        "repository": publisher.REPOSITORY, "source_sha": SHA, "source_dirty": False,
        "architecture": "x86_64", "archive_bytes": archive.stat().st_size,
        "archive_sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
    }
    receipt = archive.with_suffix(".json")
    receipt.write_text(json.dumps(metadata), encoding="utf-8")
    return archive, receipt


def macos_package(directory, macos_binary=b"immutable tested macOS package"):
    directory.mkdir(parents=True, exist_ok=True)
    archive = directory / f"deductum-1.9.0-{SHA[:12]}-macos-universal.zip"
    with zipfile.ZipFile(archive, "w") as contents:
        contents.writestr("deductum/deductum", macos_binary)
    metadata = {
        "repository": publisher.REPOSITORY, "source_sha": SHA, "source_dirty": False,
        "architecture": "universal", "architectures": ["arm64", "x86_64"],
        "archive_bytes": archive.stat().st_size,
        "archive_sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
    }
    receipt = archive.with_suffix(".json")
    receipt.write_text(json.dumps(metadata), encoding="utf-8")
    return archive, receipt


def linux_package(directory, public_scan=[]):
    directory.mkdir(parents=True, exist_ok=True)
    archive = directory / f"deductum-{SHA[:12]}-linux-x86_64.flatpak"
    archive.write_bytes(b"immutable tested Linux package")
    receipt = archive.with_suffix(".json")
    receipt.write_text(json.dumps({
        "repository": publisher.REPOSITORY, "source_sha": SHA, "source_dirty": False,
        "run_id": 123, "app_id": "dev.twright.Deductum", "architecture": "x86_64",
        "runtime": "org.freedesktop.Platform/x86_64/25.08", "archive_bytes": archive.stat().st_size,
        "archive_sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
        "public_scan": public_scan,
    }))
    return archive, receipt


class RunTests(unittest.TestCase):
    def test_accepts_only_completed_successful_main_ci_pushes(self):
        self.assertEqual(publisher.validate_run(RUN, 123), SHA)
        public = {"repository": {"full_name": publisher.REPOSITORY, "private": False}}
        self.assertEqual(publisher.validate_run({**RUN, **public}, 123), SHA)
        changes = [
            {"id": 124}, {"repository": {"full_name": "elsewhere/deductum", "private": True}},
            {"repository": {"full_name": "elsewhere/deductum", "private": False}},
            {"repository": {"full_name": publisher.REPOSITORY}},
            {"repository": {"full_name": publisher.REPOSITORY, "private": None}},
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
    def test_linux_receipt_matches_exact_source_run_runtime_and_bundle(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            assets = linux_package(root)
            self.assertEqual(publisher.validate_linux_package(root, SHA, 123), assets)
            receipt = json.loads(assets[1].read_text())
            for field, value in [("source_sha", "b" * 40), ("run_id", 124), ("app_id", "other.App"),
                                 ("source_dirty", True), ("runtime", "other.Platform/x86_64/1"),
                                 ("archive_sha256", "0" * 64)]:
                with self.subTest(field=field), self.assertRaises(ValueError):
                    assets[1].write_text(json.dumps({**receipt, field: value}))
                    publisher.validate_linux_package(root, SHA, 123)

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


class MacosPackageTests(unittest.TestCase):
    def test_checks_exact_revision_clean_build_architectures_size_and_digest(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            assets = macos_package(root)
            self.assertEqual(publisher.validate_macos_package(root, SHA), assets)
            receipt = json.loads(assets[1].read_text())
            for field, value in [
                ("repository", "elsewhere/deductum"), ("source_sha", "b" * 40),
                ("source_dirty", True), ("source_dirty", None), ("architecture", "arm64"),
                ("architectures", ["arm64"]), ("architectures", ["x86_64", "arm64"]),
                ("archive_bytes", 0), ("archive_sha256", "0" * 64),
            ]:
                with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                    assets[1].write_text(json.dumps({**receipt, field: value}))
                    publisher.validate_macos_package(root, SHA)
            assets[1].write_text(json.dumps(receipt))
            assets[0].write_bytes(b"tampered tested package")
            with self.assertRaises(ValueError):
                publisher.validate_macos_package(root, SHA)

    def test_refuses_missing_ambiguous_or_wrongly_named_packages(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with self.assertRaises(ValueError):
                publisher.validate_macos_package(root, SHA)
            archive, _ = macos_package(root)
            extra = root / "other.zip"
            extra.write_bytes(b"ambiguous")
            with self.assertRaises(ValueError):
                publisher.validate_macos_package(root, SHA)
            extra.unlink()
            for name in (f"deductum-1.9.0-{'b' * 12}-macos-universal.zip",
                         f"deductum-1.9.0-{SHA[:12]}-windows-x86_64.zip",
                         f"deductum-1.9.0-{SHA[:12]}-macos-universal-dev.zip"):
                renamed = archive.rename(root / name)
                with self.subTest(name=name), self.assertRaises(ValueError):
                    publisher.validate_macos_package(root, SHA)
                archive = renamed


class PublicationTests(unittest.TestCase):
    def setUp(self):
        self.run = copy.deepcopy(RUN)
        self.release = None
        self.uploaded = {}
        self.calls = []
        self.head = SHA
        self.plan = {"tag": "v0.2.0", "notes": "Features\n- add Windows", "reserved": False}
        self.tags = {}
        self.linux_scan = []
        self.windows_binary = b"immutable tested package"
        self.macos_binary = b"immutable tested macOS package"
        self.missing_artifact = None
        self.notes = None
        self.enterContext(patch.object(publisher.release_policy, "plan", side_effect=lambda sha, **kwargs: self.plan))
        self.enterContext(patch("builtins.print"))

    def publish(self):
        with tempfile.TemporaryDirectory() as temporary:
            staged = publisher.stage(123, temporary)
            if staged:
                publisher.finish(123, staged["tag"], temporary)
            return staged

    def gh(self, *args):
        self.calls.append(args)
        if args[:2] == ("api", f"repos/{publisher.REPOSITORY}/actions/runs/123"):
            return json.dumps(self.run)
        if args[:2] == ("api", f"repos/{publisher.REPOSITORY}/git/ref/heads/main"):
            return json.dumps({"object": {"sha": self.head}})
        if args[:2] == ("run", "download"):
            directory = Path(args[args.index("--dir") + 1])
            name = args[args.index("--name") + 1]
            if name == self.missing_artifact:
                raise subprocess.CalledProcessError(1, ["gh", "run", "download"])
            if name == "linux-x86_64":
                linux_package(directory, self.linux_scan)
            elif name == "macos-universal":
                macos_package(directory, self.macos_binary)
            else:
                package(directory, self.windows_binary)
        elif args[:3] == ("api", "--method", "POST"):
            tag = self.plan["tag"]
            if tag in self.tags:
                raise subprocess.CalledProcessError(1, ["gh", "api"])
            self.tags[tag] = SHA
        elif args[:1] == ("api",) and "/git/ref/tags/" in args[1]:
            return json.dumps({"object": {"sha": self.tags[self.plan["tag"]]}})
        elif args[:1] == ("api",) and any("/tags?" in str(value) for value in args):
            return "\n".join(json.dumps([tag, sha]) for tag, sha in self.tags.items())
        elif args[:1] == ("api",):
            return json.dumps(self.release) if self.release else ""
        elif args[:2] == ("attestation", "verify"):
            self.assertTrue(self.release["draft"], "attestation precedes visibility")
        elif args[:2] == ("release", "create"):
            self.notes = Path(args[args.index("--notes-file") + 1]).read_text()
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
            self.assertEqual(len(self.release["assets"]), 7, "publish only after all three packages, receipts and checksums exist")
            self.latest = next(arg for arg in args if str(arg).startswith("--latest="))
            self.release["draft"] = False
        else:
            self.fail(f"Unexpected gh call: {args}")
        return ""

    def test_only_the_highest_version_becomes_latest(self):
        with patch.object(publisher, "gh", self.gh):
            self.publish()
            self.assertEqual(self.latest, "--latest=true")
            self.tags["v9.0.0"] = "f" * 40
            self.assertFalse(publisher.newest_version("v0.2.0"))
            self.assertTrue(publisher.newest_version("v9.0.0"))

    def test_creates_complete_release_and_retries_without_replacing_assets(self):
        with patch.object(publisher, "gh", self.gh):
            self.publish()
            self.assertFalse(self.release["draft"])
            self.assertEqual(self.release["tag_name"], "v0.2.0")
            self.assertEqual(self.tags, {"v0.2.0": SHA})
            self.assertEqual(len(self.uploaded), 7)
            self.assertIn("deductum-v0.2.0-macos-universal.zip", self.uploaded)
            self.assertIn("deductum-v0.2.0-macos-universal.json", self.uploaded)
            sums = self.uploaded["SHA256SUMS"].decode()
            for name in ("macos-universal.zip", "macos-universal.json"):
                self.assertIn(f"deductum-v0.2.0-{name}", sums)
            self.assertIn("deductum-v0.2.0-linux-x86_64.flatpak", self.uploaded)
            self.assertIn("deductum-v0.2.0-windows-x86_64.zip", self.uploaded)
            self.calls.clear()
            self.publish()
            self.assertFalse(any(call[:2] in [("release", "upload"), ("release", "edit"),
                                             ("release", "create")] for call in self.calls))

    def test_recovers_partially_uploaded_draft_without_overwriting_it(self):
        def interrupted(*args):
            if args[:2] == ("release", "upload") and len(self.uploaded) == 1:
                raise subprocess.CalledProcessError(1, ["gh", "release", "upload"])
            return self.gh(*args)

        with patch.object(publisher, "gh", interrupted), self.assertRaises(subprocess.CalledProcessError):
            self.publish()
        self.assertTrue(self.release["draft"])
        self.calls.clear()
        with patch.object(publisher, "gh", self.gh):
            self.publish()
        self.assertFalse(self.release["draft"])
        self.assertEqual(sum(call[:2] == ("release", "upload") for call in self.calls), 6)

    def test_refuses_to_change_a_published_package(self):
        with patch.object(publisher, "gh", self.gh):
            self.publish()
            name = next(iter(self.uploaded))
            self.uploaded[name] = b"changed release asset"
            with self.assertRaisesRegex(ValueError, "immutable"):
                self.publish()

    def test_a_release_needs_all_three_platforms(self):
        for missing in ("windows-x86_64", "linux-x86_64", "macos-universal"):
            self.missing_artifact = missing
            with self.subTest(missing=missing), patch.object(publisher, "gh", self.gh), \
                    self.assertRaises(subprocess.CalledProcessError):
                self.publish()
            self.assertEqual(self.tags, {})
            self.assertIsNone(self.release)

    def test_release_notes_list_the_macos_package(self):
        with patch.object(publisher, "gh", self.gh):
            self.publish()
        self.assertIn("macOS universal", self.notes)
        self.assertIn("xattr -dr com.apple.quarantine", self.notes)
        self.assertEqual(publisher.release_scan.text_findings(self.notes), [])

    def test_never_downloads_or_publishes_from_failed_ci(self):
        self.run["conclusion"] = "failure"
        with patch.object(publisher, "gh", self.gh), self.assertRaises(ValueError):
            self.publish()
        self.assertEqual(len(self.calls), 1)

    def test_no_release_for_docs_only_or_superseded_main(self):
        for plan, head in [(None, SHA), (self.plan, "b" * 40)]:
            self.plan, self.head = plan, head
            self.calls.clear()
            with patch.object(publisher, "gh", self.gh):
                self.publish()
            self.assertFalse(any(call[0] == "run" or call[:2] == ("release", "create") for call in self.calls))

    def test_reserved_release_can_finish_after_a_newer_main_push(self):
        self.plan["reserved"] = True
        self.tags["v0.2.0"] = SHA
        self.head = "b" * 40
        with patch.object(publisher, "gh", self.gh):
            self.publish()
        self.assertFalse(self.release["draft"])

    def test_tag_reservation_cannot_overwrite_another_source(self):
        self.tags["v0.2.0"] = "b" * 40
        with patch.object(publisher, "gh", self.gh), self.assertRaisesRegex(ValueError, "another source"):
            self.publish()
        self.assertEqual(self.tags["v0.2.0"], "b" * 40)
        self.assertIsNone(self.release)

    def test_recalculates_after_a_concurrent_ancestor_reserves_the_version(self):
        first_plan = dict(self.plan)
        next_plan = dict(self.plan, tag="v0.2.1", notes="Fixes\n- repair navigation")
        self.tags["v0.2.0"] = "b" * 40

        def calculate(sha):
            plan = first_plan if len([call for call in self.calls if call[:3] == ("api", "--method", "POST")]) == 0 else next_plan
            self.plan = plan
            return plan

        with patch.object(publisher, "gh", self.gh), patch.object(publisher, "release_plan", calculate):
            self.publish()
        self.assertEqual(self.tags["v0.2.0"], "b" * 40)
        self.assertEqual(self.tags["v0.2.1"], SHA)
        self.assertEqual(self.release["tag_name"], "v0.2.1")


class PublicPublicationTests(PublicationTests):
    """Every publication rule above also holds in a public repository."""

    decision = publisher.GITHUB_ATTESTATION

    def setUp(self):
        super().setUp()
        self.run["repository"]["private"] = False
        config = {"public_signing": self.decision}
        self.enterContext(patch.object(publisher.release_config, "load", return_value=config))
        self.config = config

    def attestations(self):
        return [call[2] for call in self.calls if call[:2] == ("attestation", "verify")]

    def test_every_file_is_attested_before_the_release_is_visible(self):
        with patch.object(publisher, "gh", self.gh):
            staged = self.publish()
        self.assertTrue(staged["attest"])
        self.assertFalse(self.release["draft"])
        self.assertEqual({Path(path).name for path in self.attestations()}, set(self.uploaded))
        self.assertEqual(len(self.uploaded), 7)

    def test_attestation_identifies_this_repository_and_release_workflow(self):
        with patch.object(publisher, "gh", self.gh):
            self.publish()
        verify = next(call for call in self.calls if call[:2] == ("attestation", "verify"))
        self.assertEqual(verify[verify.index("--repo") + 1], publisher.REPOSITORY)
        self.assertEqual(verify[verify.index("--signer-workflow") + 1],
                         f"{publisher.REPOSITORY}/.github/workflows/release-binaries.yml")

    def test_unverifiable_attestation_leaves_the_release_a_draft(self):
        def refused(*args):
            if args[:2] == ("attestation", "verify"):
                raise subprocess.CalledProcessError(1, ["gh", "attestation"])
            return self.gh(*args)

        with patch.object(publisher, "gh", refused), patch.object(publisher.time, "sleep"), \
                self.assertRaises(subprocess.CalledProcessError):
            self.publish()
        self.assertTrue(self.release["draft"])

    def test_undecided_signing_publishes_nothing(self):
        self.config["public_signing"] = None
        with patch.object(publisher, "gh", self.gh):
            self.assertIsNone(self.publish())
        self.assertEqual(len(self.calls), 1, "only the run is read")
        self.assertEqual(self.tags, {})
        self.assertIsNone(self.release)

    def test_unknown_signing_identity_publishes_nothing(self):
        self.config["public_signing"] = "a key nobody recorded"
        with patch.object(publisher, "gh", self.gh), self.assertRaisesRegex(ValueError, "public_signing"):
            self.publish()
        self.assertEqual(self.tags, {})

    def test_refuses_public_packages_naming_household_systems_before_reserving(self):
        cases = [
            ("linux_scan", ["tailnet name"]),
            ("linux_scan", None),
            ("windows_binary", b"ssh machine.tail0a1b2c.ts.net"),
            ("windows_binary", b"http://neo:8099/repo"),
            ("windows_binary", b"/mnt/disk-array/Share"),
            ("macos_binary", b"ssh machine.tail0a1b2c.ts.net"),
            ("macos_binary", b"/Users/x/../home/twright/source"),
        ]
        for field, value in cases:
            original = getattr(self, field)
            setattr(self, field, value)
            with self.subTest(field=field, value=value), patch.object(publisher, "gh", self.gh), \
                    self.assertRaises(ValueError):
                self.publish()
            setattr(self, field, original)
            self.assertEqual(self.tags, {})
            self.assertIsNone(self.release)

    def test_notes_withhold_household_changes_and_explain_verification(self):
        self.plan["notes"] = ("Features\n- add Windows\n- speed up the neo runner\n"
                              "- publish to home-apps\n- reach machine.tail0a1b2c.ts.net")
        with patch.object(publisher, "gh", self.gh):
            self.publish()
        self.assertIn("add Windows", self.notes)
        self.assertIn("3 household infrastructure changes are not listed", self.notes)
        self.assertIn("gh attestation verify", self.notes)
        self.assertEqual(publisher.release_scan.text_findings(self.notes), [])


class PrivateVisibilityTests(PublicationTests):
    """A private repository keeps publishing with no attestation or public checks."""

    def test_private_release_needs_no_attestation_or_scan(self):
        self.linux_scan = ["tailnet name"]
        with patch.object(publisher, "gh", self.gh):
            staged = self.publish()
        self.assertFalse(staged["attest"])
        self.assertFalse(self.release["draft"])
        self.assertFalse(any(call[0] == "attestation" for call in self.calls))
        self.assertNotIn("gh attestation verify", self.notes)
        self.assertEqual(publisher.release_scan.text_findings(self.notes), [])


class ScanTests(unittest.TestCase):
    scan = publisher.release_scan

    def test_package_rules_find_household_names_but_not_ordinary_text(self):
        for data in (b"x.tail57c02a.ts.net", b"100.64.0.1", b"100.127.255.2", b"http://neo:8101/",
                     b"http://100.101.2.3/", b"100.101.2.3/32", b"100.101.2.3:22", b"[100.101.2.3]",
                     b"/mnt/disk-array/x", b"/home/twright/x", b"/var/home/twright/x",
                     b"/var/lib/neo-infrastructure/x"):
            with self.subTest(data=data):
                self.assertTrue(self.scan.findings(data))
        # The app recognises tailnet names, ships a neo-dark theme and has an app ID.
        for data in (b'host.ends_with(".ts.net")', b"neo-dark", b"dev.twright.Deductum",
                     b"100.63.0.1", b"100.128.0.1", b"1.100.64.0.1",
                     # CPython's ipaddress module, bundled in the macOS package, names the range
                     b"IPv4Network('100.64.0.0/10')", b"``100.64.0.0/10``"):
            with self.subTest(data=data):
                self.assertEqual(self.scan.findings(data), [])

    def test_findings_are_labels_never_the_matching_text(self):
        self.assertEqual(self.scan.findings(b"machine.tail0a1b2c.ts.net"), ["tailnet name"])

    def test_scans_archive_names_and_contents_and_trees(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for name, content in [("plain.txt", b"neo:8099"), ("neo:8099", b"plain")]:
                archive = root / "package.zip"
                with zipfile.ZipFile(archive, "w") as contents:
                    contents.writestr(name, content)
                self.assertEqual(self.scan.scan_zip(archive), ["household service"])
            tree = root / "tree"
            (tree / "bin").mkdir(parents=True)
            (tree / "bin/clean").write_bytes(b"clean")
            self.assertEqual(self.scan.scan_tree(tree), [])
            (tree / "bin/app").write_bytes(b"\0/home/twright/source\0")
            self.assertEqual(self.scan.scan_tree(tree), ["household path"])


if __name__ == "__main__":
    unittest.main()
