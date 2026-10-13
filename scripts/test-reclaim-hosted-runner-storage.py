#!/usr/bin/env python3
"""Exercise cleanup authorization and Docker boundaries without host deletion."""
import contextlib
import importlib.util
import io
from pathlib import Path
import struct
import unittest

spec = importlib.util.spec_from_file_location("reclaim", Path(__file__).with_name("reclaim-hosted-runner-storage.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
JOB = "a" * 12
IMAGE = "sha256:" + "b" * 64
CHILD = "c" * 64
HOSTED = {"GITHUB_ACTIONS": "true", "RUNNER_ENVIRONMENT": "github-hosted", "RUNNER_OS": "Linux"}


class FakeAPI:
    def __init__(self, status=0):
        self.calls = []
        self.status = status
        self.current = {"Id": JOB + "a" * 52, "Image": IMAGE,
                        "Config": {"Env": ["GH_TOKEN=must-never-be-logged"]}}

    def request(self, method, path, payload=None):
        self.calls.append((method, path, payload))
        if path.endswith("/json"):
            return self.current
        if path.startswith("/containers/create?"):
            return {"Id": CHILD}
        if "/wait?" in path:
            return {"StatusCode": self.status}
        return None

    def logs(self, identity):
        assert identity == CHILD
        return "Reclaimed unused toolchain: /ci-reclaim/android\n"


class Boundaries(unittest.TestCase):
    def run_cleanup(self, api, environment=None, hostname=JOB):
        reports = []
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            module.reclaim(api, HOSTED if environment is None else environment, hostname, reports.append)
        return reports, output.getvalue()

    def test_local_and_self_hosted_refuse_before_docker(self):
        for environment, hostname in [({}, JOB), (HOSTED, "neo"),
                                      ({**HOSTED, "RUNNER_ENVIRONMENT": "self-hosted"}, JOB),
                                      ({**HOSTED, "GITHUB_ACTIONS": "false"}, JOB),
                                      ({**HOSTED, "RUNNER_OS": "Windows"}, JOB)]:
            with self.subTest(environment=environment, hostname=hostname):
                api = FakeAPI()
                with self.assertRaises(RuntimeError):
                    self.run_cleanup(api, environment, hostname)
                self.assertEqual(api.calls, [])

    def test_current_container_identity_is_required(self):
        for change in [{"Id": "d" * 64}, {"Image": "untrusted:latest"}]:
            with self.subTest(change=change):
                api = FakeAPI()
                api.current.update(change)
                with self.assertRaises(RuntimeError):
                    self.run_cleanup(api)
                self.assertEqual(len(api.calls), 1)

    def test_only_fixed_toolchains_are_mounted_without_credentials_or_capabilities(self):
        api = FakeAPI()
        reports, output = self.run_cleanup(api)
        creation = api.calls[1]
        self.assertEqual(creation[0], "POST")
        self.assertTrue(creation[1].startswith("/containers/create?name=household-ci-reclaim-"))
        request = creation[2]
        self.assertEqual(request["Image"], IMAGE)
        self.assertEqual(request["User"], "0:0")
        self.assertEqual(request["Entrypoint"], ["python3", "-c"])
        self.assertEqual(request["Cmd"], [module.HELPER])
        self.assertEqual(request["Env"], ["PYTHONDONTWRITEBYTECODE=1"])
        self.assertEqual(request["HostConfig"], {
            "Binds": ["/usr/local/lib/android:/ci-reclaim/android:rw",
                      "/usr/share/dotnet:/ci-reclaim/dotnet:rw",
                      "/usr/share/swift:/ci-reclaim/swift:rw"],
            "ReadonlyRootfs": True, "NetworkMode": "none", "Privileged": False,
            "CapDrop": ["ALL"], "SecurityOpt": ["no-new-privileges"],
        })
        self.assertEqual(reports, ["before cleanup", "after cleanup"])
        self.assertNotIn("must-never-be-logged", output)
        self.assertEqual(api.calls[-1], ("DELETE", "/containers/" + CHILD + "?force=1", None))
        self.assertFalse(any("/images/" in path for _, path, _ in api.calls))

    def test_failed_helper_is_removed_and_storage_reported(self):
        api = FakeAPI(status=1)
        reports = []
        with contextlib.redirect_stdout(io.StringIO()), self.assertRaises(RuntimeError):
            module.reclaim(api, HOSTED, JOB, reports.append)
        self.assertEqual(reports, ["before cleanup", "after cleanup"])
        self.assertEqual(api.calls[-1][0], "DELETE")

    def test_logs_accept_only_bounded_docker_frames(self):
        api = module.DockerAPI()
        message = b"safe cleanup output\n"
        api.raw = lambda *args: b"\x01\0\0\0" + struct.pack(">I", len(message)) + message
        self.assertEqual(api.logs(CHILD), message.decode())
        for wire in [b"TOKEN=must-never-be-logged", b"\x01\0\0\0" + struct.pack(">I", 2**20 + 1)]:
            api.raw = lambda *args: wire
            with self.assertRaisesRegex(RuntimeError, "Invalid Docker log"):
                api.logs(CHILD)


if __name__ == "__main__":
    unittest.main()
