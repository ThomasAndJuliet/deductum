#!/usr/bin/env python3
"""Versioning follows Conventional Commits and only adopted reachable history."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

import release_policy as policy


class VersionTests(unittest.TestCase):
    def test_semver_bumps_and_non_release_commits(self):
        cases = [
            (["fix(editor): keep the caret"], (1, 2, 4)),
            (["FIX(editor): keep the caret"], (1, 2, 4)),
            (["perf: reduce retained layouts"], (1, 2, 4)),
            (["feat: add Windows", "fix: a typo"], (1, 3, 0)),
            (["feat!: change the protocol"], (2, 0, 0)),
            (["refactor(host)!: change the protocol"], (2, 0, 0)),
            (["fix: change format\n\nBREAKING CHANGE: old states need conversion"], (2, 0, 0)),
            (["feat: change format\n\nBREAKING-CHANGE: old states need conversion"], (2, 0, 0)),
            (["docs: write installation instructions", "test: add coverage", "chore: dependencies"], None),
            (["Merge pull request #12", "legacy message"], None),
        ]
        for messages, expected in cases:
            with self.subTest(messages=messages):
                self.assertEqual(policy.next_version((1, 2, 3), messages), expected)
        self.assertEqual(policy.next_version((0, 1, 0), ["feat!: incompatible change"]), (1, 0, 0))


class HistoryTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        previous = Path.cwd()
        self.addCleanup(os.chdir, previous)
        os.chdir(temporary.name)
        self.git("init", "-q", "--initial-branch=main")
        self.git("config", "user.name", "Release tests")
        self.git("config", "user.email", "release-tests@example.invalid")
        self.commit("feat: inherited Terminal history")
        self.git("tag", "v99.0.0")
        self.bootstrap = self.commit("chore: adopt Deductum releases")
        Path("release-config.json").write_text(json.dumps({"bootstrap_sha": self.bootstrap, "initial_version": "0.1.0"}))

    def git(self, *args):
        return subprocess.check_output(["git", *args], text=True, stderr=subprocess.DEVNULL).strip()

    def commit(self, message):
        self.git("-c", "commit.gpgsign=false", "commit", "--allow-empty", "-qm", message)
        return self.git("rev-parse", "HEAD")

    def test_bootstraps_without_releasing_inherited_history(self):
        self.assertIsNone(policy.plan(self.bootstrap))
        sha = self.commit("feat: add proof windows")
        plan = policy.plan(sha)
        self.assertEqual(plan["tag"], "v0.2.0")
        self.assertIn("add proof windows", plan["notes"])
        self.assertNotIn("inherited", plan["notes"])

    def test_bumps_from_reachable_release_and_resumes_reserved_tag(self):
        sha = self.commit("feat: add proof windows")
        self.git("tag", "v0.2.0", sha)
        resumed = policy.plan(sha)
        self.assertEqual(resumed["tag"], "v0.2.0")
        self.assertTrue(resumed["reserved"])
        self.assertIn("add proof windows", resumed["notes"])
        sha = self.commit("fix: repair navigation")
        plan = policy.plan(sha)
        self.assertEqual(plan["tag"], "v0.2.1")
        self.assertNotIn("proof windows", plan["notes"])

    def test_unrelated_and_prerelease_tags_do_not_change_stable_version(self):
        self.git("switch", "-qc", "other")
        other = self.commit("feat!: unrelated work")
        self.git("tag", "v10.0.0", other)
        self.git("switch", "-q", "main")
        sha = self.commit("fix: repair navigation")
        self.git("tag", "v0.9.0-alpha.1", sha)
        self.assertEqual(policy.plan(sha)["tag"], "v0.1.1")

    def test_documentation_waits_for_next_user_visible_change(self):
        self.assertIsNone(policy.plan(self.commit("docs: explain installation")))
        plan = policy.plan(self.commit("fix: repair navigation"))
        self.assertEqual(plan["tag"], "v0.1.1")
        self.assertNotIn("explain installation", plan["notes"])

    def test_remote_reservation_is_used_without_fetching_or_mutating_git_tags(self):
        previous = self.commit("feat: add a search tool")
        sha = self.commit("fix: repair navigation")
        plan = policy.plan(sha, tags=[("v0.2.0", previous), ("v9.0.0", "f" * 40)])
        self.assertEqual(plan["tag"], "v0.2.1")
        self.assertNotIn("search tool", plan["notes"])
        self.assertNotIn("v0.2.0", self.git("tag").splitlines())

    def test_merged_feature_commits_are_counted(self):
        self.git("switch", "-qc", "feature")
        self.commit("feat(proofs): add a search tool")
        self.git("switch", "-q", "main")
        self.git("-c", "commit.gpgsign=false", "merge", "--no-ff", "-qm", "Merge feature", "feature")
        self.assertEqual(policy.plan(self.git("rev-parse", "HEAD"))["tag"], "v0.2.0")


if __name__ == "__main__":
    unittest.main()
