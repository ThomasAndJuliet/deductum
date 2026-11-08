#!/usr/bin/env python3
"""Submission refuses untrusted runs and cannot discard an unaccepted artifact."""
import contextlib
import importlib.util
import io
import os
from pathlib import Path
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("client", Path(__file__).with_name("submit-home-apps.py"))
client = importlib.util.module_from_spec(spec)
spec.loader.exec_module(client)
REPO = client.release_config.repository()
APP = client.APP_ID
ARTIFACT = client.ARTIFACT
RUN = {"id": 11, "run_number": 2, "event": "push", "head_branch": "main",
       "status": "completed", "conclusion": "success", "path": ".github/workflows/ci.yml",
       "repository": {"id": 42}, "head_repository": {"id": 42}, "head_sha": "a" * 40}
READY = {"repository": REPO, "run_id": 11, "sha": "a" * 40, "phase": "ready",
         "digest": "sha256:" + "b" * 64, "artifact_id": 7}
SIGNED = dict(READY, phase="published",
              archive_receipt={"app": APP, "status": "published", "commit": "c" * 64,
                               "ci": {"sha": "a" * 40, "run_id": 11}},
              stable_receipt={"app": APP, "status": "published", "commit": "d" * 64,
                              "ci": {"sha": "a" * 40, "run_id": 11}})


class FakeAPI:
    repository = REPO

    def __init__(self):
        self.deleted = []

    def collection(self, path, field):
        return iter([{"id": 7, "name": ARTIFACT}])

    def call(self, path, method=None):
        self.deleted.append((path, method))


def submit(api, response):
    side = {"side_effect": response} if isinstance(response, Exception) else {"return_value": response}
    with patch.dict(os.environ, {"GITHUB_TOKEN": "fixture-token"}), patch.object(client, "request", **side), \
            contextlib.redirect_stdout(io.StringIO()):
        return client.submit(api, RUN, client.PUBLISH_URL, APP, ARTIFACT, token_supplier=lambda: "fixture-jwt",
                             sleeper=lambda _: None)


class SubmissionTests(unittest.TestCase):
    def test_only_completed_main_push_ci_of_this_repository_is_eligible(self):
        self.assertTrue(client.eligible(RUN, REPO, 42, "main"))
        for key, value in [("head_branch", "feature"), ("event", "pull_request"), ("status", "in_progress"),
                           ("conclusion", "failure"), ("head_repository", {"id": 43}), ("repository", {"id": 43}),
                           ("path", ".github/workflows/other.yml"), ("head_sha", "x"), ("id", True),
                           ("run_number", False)]:
            with self.subTest(field=key):
                self.assertFalse(client.eligible(dict(RUN, **{key: value}), REPO, 42, "main"))

    def test_credentials_go_only_to_the_fixed_ingress_with_no_redirects(self):
        self.assertEqual(client.PUBLISH_URL, "http://neo:8101/v1/releases")
        with self.assertRaises(client.Failure):
            client.NoRedirect().redirect_request(None, None, 302, "", {}, "https://elsewhere.invalid")

    def test_refused_or_mismatched_acceptance_keeps_the_artifact(self):
        for response in [dict(READY, sha="c" * 40), dict(READY, run_id=12), dict(READY, repository="other/app"),
                         dict(READY, phase=None), dict(READY, digest="bad"), client.Failure("refused")]:
            api = FakeAPI()
            with self.subTest(response=response), self.assertRaises(client.Failure):
                submit(api, response)
            self.assertEqual(api.deleted, [])

    def test_artifact_is_removed_after_acceptance_and_signature_is_required(self):
        api = FakeAPI()
        self.assertEqual(submit(api, SIGNED), SIGNED)
        self.assertEqual(api.deleted, [("/actions/artifacts/7", "DELETE")])
        wrong = dict(SIGNED, stable_receipt=dict(SIGNED["stable_receipt"], app="other.App"))
        with self.assertRaises(client.Failure):
            submit(FakeAPI(), wrong)

    def test_unsigned_build_fails_visibly_after_waiting(self):
        with self.assertRaisesRegex(client.Failure, "signing is still pending"):
            submit(FakeAPI(), READY)

    def test_refuses_excess_network_access_and_accepts_blocked_ports(self):
        with patch.object(client.socket, "create_connection", side_effect=OSError):
            client.require_narrow_network()
        with patch.object(client.socket, "create_connection"), self.assertRaisesRegex(client.Failure, "excess"):
            client.require_narrow_network()

    def test_refuses_a_copy_of_this_repository_before_any_credential_is_used(self):
        env = {"GITHUB_REPOSITORY": "someone/copy", "GITHUB_TOKEN": "fixture-token"}
        with patch.dict(os.environ, env), patch.object(client, "request") as request, \
                self.assertRaisesRegex(client.Failure, "release-config"):
            client.main()
        request.assert_not_called()


if __name__ == "__main__":
    unittest.main()
