#!/usr/bin/env python3
"""CI's trust decisions and the workflow shape that depends on them."""
import importlib.util
import json
from pathlib import Path
import re
import unittest

import yaml

import release_config


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), Path(__file__).with_name(name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


trust = load("ci-trust")
validated = load("ci-already-validated")
ROOT = Path(__file__).resolve().parents[1]
WORKFLOWS = ROOT / ".github/workflows"
REPOSITORY = "owner/app"


def event(private=True, fork=False, head="owner/app"):
    payload = {"repository": {"private": private, "fork": fork}}
    if head != "none":
        payload["pull_request"] = {"head": {"repo": {"full_name": head} if head else None}}
    return payload


def classify(name, payload, repository=REPOSITORY, runner=""):
    return trust.classify(name, payload, repository, REPOSITORY, runner)


class ClassifyTests(unittest.TestCase):
    def test_own_private_runs_use_household_runners(self):
        for name, payload in [("push", event(head="none")), ("workflow_dispatch", event(head="none")),
                              ("pull_request", event())]:
            with self.subTest(name=name):
                self.assertEqual(classify(name, payload),
                                 {"trusted": "true", "runner": "neo-ubuntu", "flatpak": "neo"})

    def test_ci_runner_variable_moves_private_runs_to_hosted(self):
        self.assertEqual(classify("push", event(head="none"), runner="hosted"),
                         {"trusted": "true", "runner": "ubuntu-24.04", "flatpak": "hosted"})

    def test_public_repository_never_selects_household_runners(self):
        for name, payload in [("push", event(private=False, head="none")),
                              ("pull_request", event(private=False))]:
            with self.subTest(name=name):
                self.assertEqual(classify(name, payload),
                                 {"trusted": "true", "runner": "ubuntu-24.04", "flatpak": "hosted"})

    def test_untrusted_runs_get_hosted_runners_and_no_deploy_keys(self):
        cases = {
            "fork pull request": ("pull_request", event(private=False, head="attacker/app"), REPOSITORY),
            "pull request from a deleted fork": ("pull_request", event(private=False, head=None), REPOSITORY),
            "private base with a fork head": ("pull_request", event(head="attacker/app"), REPOSITORY),
            "pull request event without a head": ("pull_request", {"repository": {"private": False, "fork": False}}, REPOSITORY),
            "push inside a fork": ("push", event(private=False, fork=True, head="none"), "attacker/app"),
            "copy under another name": ("push", event(head="none"), "owner/copy"),
            "fork named like the repository": ("push", event(fork=True, head="none"), REPOSITORY),
            "event without repository details": ("push", {}, REPOSITORY),
            "pull_request_target": ("pull_request_target", event(), REPOSITORY),
            "unknown event": ("issue_comment", event(head="none"), REPOSITORY),
        }
        for label, (name, payload, repository) in cases.items():
            with self.subTest(label):
                self.assertEqual(classify(name, payload, repository),
                                 {"trusted": "false", "runner": "ubuntu-24.04", "flatpak": "hosted"})


class ValidatedRunTests(unittest.TestCase):
    PATH = ".github/workflows/ci.yml"

    def run_row(self, **change):
        return {"id": 5, "path": self.PATH, "head_sha": "a", "conclusion": "success", **change}

    def test_finds_earliest_other_successful_run_of_this_workflow_and_commit(self):
        rows = [self.run_row(id=9), self.run_row(id=5), self.run_row(id=10)]
        self.assertEqual(validated.validated_run(rows, self.PATH, "a", 10), 5)

    def test_ignores_this_run_other_workflows_commits_and_failures(self):
        rows = [self.run_row(id=10), self.run_row(path=".github/workflows/other.yml"),
                self.run_row(head_sha="b"), self.run_row(conclusion="failure")]
        self.assertIsNone(validated.validated_run(rows, self.PATH, "a", 10))


def jobs(name):
    return yaml.safe_load((WORKFLOWS / name).read_text())["jobs"]


def triggers(document):
    # YAML 1.1 reads the bare key `on` as True.
    return document.get("on", document.get(True))


class WorkflowShapeTests(unittest.TestCase):
    def setUp(self):
        self.ci = jobs("ci.yml")
        self.all = sorted(WORKFLOWS.glob("*.yml"))

    def test_no_workflow_uses_private_infrastructure(self):
        # A public repository cannot use actions or workflows from a private one.
        for path in self.all:
            with self.subTest(path=path.name):
                self.assertEqual(re.findall(r"uses:\s*(ThomasAndJuliet/\S+)", path.read_text()), [])

    def test_no_workflow_runs_pull_request_code_with_repository_privileges(self):
        for path in self.all:
            with self.subTest(path=path.name):
                self.assertNotIn("pull_request_target", path.read_text())

    def test_actions_are_pinned_to_commits(self):
        for path in self.all:
            for action in re.findall(r"uses:\s*(\S+)", path.read_text()):
                if action.startswith("./"):
                    continue
                with self.subTest(path=path.name, action=action):
                    self.assertRegex(action, r"@[0-9a-f]{40}$")

    def test_ci_token_stays_read_only(self):
        document = yaml.safe_load((WORKFLOWS / "ci.yml").read_text())
        self.assertEqual(document["permissions"], {"contents": "read"})
        for name, job in self.ci.items():
            for scope, level in (job.get("permissions") or {}).items():
                with self.subTest(job=name, scope=scope):
                    self.assertEqual(level, "read")

    def test_trust_job_itself_is_hosted_and_uses_no_secret(self):
        self.assertEqual(self.ci["trust"]["runs-on"], "ubuntu-24.04")
        self.assertNotIn("secrets.", json.dumps(self.ci["trust"]))

    def test_ci_uses_no_secret(self):
        self.assertNotIn("secrets.", (WORKFLOWS / "ci.yml").read_text())

    def test_crates_are_fetched_without_credentials(self):
        for path in (ROOT / "Cargo.toml", ROOT / "Cargo.lock", ROOT / "packaging/flatpak/cargo-sources.json"):
            with self.subTest(path=path.name):
                self.assertNotIn("ssh://", path.read_text())

    def test_household_runners_need_a_trusted_run(self):
        for name, job in self.ci.items():
            if re.search(r"\bneo\b|self-hosted|neo-ubuntu", json.dumps(job.get("runs-on"))):
                with self.subTest(job=name):
                    self.assertIn("trust", job["needs"])
                    self.assertIn("needs.trust.outputs.trusted == 'true'", job["if"])

    def test_runner_choices_come_from_the_trust_job(self):
        for name, job in self.ci.items():
            runner = job["runs-on"]
            with self.subTest(job=name):
                # A literal is hosted, or is the one household job its trust decision selects.
                allowed = {"ubuntu-24.04", "windows-2025", "${{ needs.trust.outputs.runner }}"}
                if name == "flatpak-neo":
                    self.assertEqual(runner, ["self-hosted", "neo"])
                    self.assertIn("needs.trust.outputs.flatpak == 'neo'", job["if"])
                else:
                    self.assertIn(runner, allowed)

    def test_home_apps_artifact_comes_only_from_main_pushes(self):
        steps = self.ci["flatpak-neo"]["steps"]
        upload = next(step for step in steps if step.get("with", {}).get("name") == "home-apps-x86_64")
        self.assertEqual(upload["if"], "github.event_name == 'push' && github.ref == 'refs/heads/main'")

    def test_publisher_is_hosted_only_and_runs_only_after_main_push_ci(self):
        document = yaml.safe_load((WORKFLOWS / "release-binaries.yml").read_text())
        job = document["jobs"]["packages"]
        self.assertEqual(document["permissions"], {"contents": "read"})
        self.assertEqual(job["runs-on"], "ubuntu-24.04")
        for clause in ("github.event.repository.fork == false", "github.ref == 'refs/heads/main'",
                       "github.event.workflow_run.event == 'push'",
                       "github.event.workflow_run.head_branch == 'main'",
                       "github.event.workflow_run.conclusion == 'success'",
                       "github.event.workflow_run.head_repository.full_name == github.repository"):
            with self.subTest(clause=clause):
                self.assertIn(clause, job["if"])
        self.assertEqual(triggers(document)["workflow_run"]["workflows"], ["CI"])

    def test_publisher_attests_between_staging_and_finishing(self):
        steps = jobs("release-binaries.yml")["packages"]["steps"]
        names = [step.get("id") for step in steps]
        self.assertLess(names.index("stage"), names.index("attest"))
        self.assertLess(names.index("attest"), names.index("finish"))
        attest = steps[names.index("attest")]
        self.assertEqual(attest["if"], "steps.stage.outputs.attest == 'true'")
        self.assertRegex(attest["uses"], r"^actions/attest@[0-9a-f]{40}$")
        checkout = next(step for step in steps if "actions/checkout" in step.get("uses", ""))
        self.assertIs(checkout["with"]["persist-credentials"], False)

    def test_home_apps_publisher_runs_hosted_only_after_main_push_ci(self):
        document = yaml.safe_load((WORKFLOWS / "release.yml").read_text())
        job = document["jobs"]["publish"]
        self.assertEqual(document["permissions"], {"contents": "read"})
        self.assertEqual(set(triggers(document)), {"workflow_run", "schedule", "workflow_dispatch"})
        self.assertEqual(job["runs-on"], "ubuntu-24.04")
        self.assertEqual(job["permissions"], {"contents": "read", "actions": "write", "id-token": "write"})
        for clause in ("github.event.repository.fork == false", "github.ref == 'refs/heads/main'",
                       "github.event.workflow_run.event == 'push'",
                       "github.event.workflow_run.head_branch == 'main'",
                       "github.event.workflow_run.conclusion == 'success'",
                       "github.event.workflow_run.head_repository.full_name == github.repository"):
            with self.subTest(clause=clause):
                self.assertIn(clause, job["if"])
        names = [step.get("id") or step.get("name") for step in job["steps"]]
        self.assertLess(names.index("discover"), names.index("Join the publishing network with a short-lived identity"))

    def test_only_publishers_hold_an_identity_token(self):
        for path in self.all:
            if path.name not in ("release.yml", "release-binaries.yml"):
                with self.subTest(path=path.name):
                    self.assertNotIn("id-token", path.read_text())


class IdentityTests(unittest.TestCase):
    def test_repository_is_named_only_in_release_config(self):
        # Install links and app metadata are documentation, updated with this file.
        name = release_config.repository()
        paths = [*(ROOT / ".github/workflows").glob("*.yml"), *(ROOT / "scripts").glob("*.py"),
                 *(ROOT / "scripts").glob("*.ps1")]
        for path in paths:
            if path.exists():
                with self.subTest(path=path.name):
                    self.assertNotIn(name, path.read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
