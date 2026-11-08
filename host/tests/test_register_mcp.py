"""Registering the deductum MCP server for Claude and Codex: it is added where missing and nothing
else is touched. Every test runs in a temporary HOME; the real configuration is never read."""
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import deductum_mcp as M  # noqa: E402
import register_mcp as R  # noqa: E402

if os.name == "nt":
    # (the app registers the server only where it starts agent sessions: not on Windows)
    raise unittest.SkipTest("MCP registration is not on Windows")

ROOT = Path("/opt/deductum")
ENTRY = {"type": "stdio", "command": "python3", "args": ["/opt/deductum/host/deductum_host.py", "mcp"], "env": {}}


class Home(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.home = Path(self._tmp.name)
        # (a CODEX_HOME of the real environment must not send a test to the real configuration)
        patcher = mock.patch.dict(os.environ, {"HOME": str(self.home)})
        patcher.start()
        os.environ.pop("CODEX_HOME", None)
        self.addCleanup(patcher.stop)
        self.addCleanup(self._tmp.cleanup)

    def claude(self):
        return json.loads((self.home / ".claude.json").read_text())


class Claude(Home):
    def test_a_machine_without_claude_gets_no_file(self):
        self.assertEqual(R.register_claude(self.home, ROOT), "skipped")
        self.assertFalse((self.home / ".claude.json").exists())

    def test_added_beside_what_is_there(self):
        other = {"type": "stdio", "command": "x", "args": [], "env": {}}
        (self.home / ".claude.json").write_text(json.dumps({"numStartups": 4, "projects": {"/p": {"a": 1}}, "mcpServers": {"marginalium": other}}))
        self.assertEqual(R.register_claude(self.home, ROOT), "added")
        data = self.claude()
        self.assertEqual(data["mcpServers"], {"marginalium": other, "deductum": ENTRY})
        self.assertEqual((data["numStartups"], data["projects"]), (4, {"/p": {"a": 1}}))

    def test_an_existing_entry_is_left_exactly_as_it_is(self):
        mine = {"type": "stdio", "command": "python3", "args": ["/home/me/dev/deductum_host.py", "mcp"], "env": {"X": "1"}}
        text = json.dumps({"mcpServers": {"deductum": mine}}, indent=4)
        (self.home / ".claude.json").write_text(text)
        self.assertEqual(R.register_claude(self.home, ROOT), "present")
        self.assertEqual((self.home / ".claude.json").read_text(), text)

    def test_a_claude_with_no_file_yet_gets_one_when_its_folder_is_there(self):
        (self.home / ".claude").mkdir()
        self.assertEqual(R.register_claude(self.home, ROOT), "added")
        self.assertEqual(self.claude(), {"mcpServers": {"deductum": ENTRY}})

    def test_a_file_that_cannot_be_read_is_not_overwritten(self):
        (self.home / ".claude.json").write_text("{ not json")
        self.assertEqual(R.register_claude(self.home, ROOT), "unreadable")
        self.assertEqual((self.home / ".claude.json").read_text(), "{ not json")
        (self.home / ".claude.json").write_text(json.dumps({"mcpServers": ["odd"]}))
        self.assertEqual(R.register_claude(self.home, ROOT), "unreadable")


class Codex(Home):
    def toml(self):
        return (self.home / ".codex" / "config.toml").read_text()

    def test_a_machine_without_codex_gets_no_file(self):
        self.assertEqual(R.register_codex(self.home, ROOT), "skipped")
        self.assertFalse((self.home / ".codex").exists())

    def test_appended_to_what_is_there(self):
        (self.home / ".codex").mkdir()
        before = 'model = "gpt-5"\n\n[projects."/p"]\ntrust_level = "trusted"\n'
        (self.home / ".codex" / "config.toml").write_text(before)
        self.assertEqual(R.register_codex(self.home, ROOT), "added")
        text = self.toml()
        self.assertTrue(text.startswith(before))
        self.assertIn('[mcp_servers.deductum]\ncommand = "python3"\nargs = ["/opt/deductum/host/deductum_host.py", "mcp"]\n', text)
        # and it is valid TOML for what reads it
        try:
            import tomllib
        except ImportError:  # Python before 3.11
            return
        self.assertEqual(tomllib.loads(text)["mcp_servers"]["deductum"], {"command": "python3", "args": ["/opt/deductum/host/deductum_host.py", "mcp"]})

    def test_registered_once_and_an_existing_table_is_left_alone(self):
        (self.home / ".codex").mkdir()
        self.assertEqual(R.register_codex(self.home, ROOT), "added")
        once = self.toml()
        self.assertEqual(R.register_codex(self.home, ROOT), "present")
        self.assertEqual(self.toml(), once)
        mine = '[mcp_servers."deductum"]\ncommand = "mine"\n\n[mcp_servers.deductum.env]\nA = "1"\n'
        (self.home / ".codex" / "config.toml").write_text(mine)
        self.assertEqual(R.register_codex(self.home, ROOT), "present")
        self.assertEqual(self.toml(), mine)

    def test_codex_home_is_where_it_is(self):
        elsewhere = self.home / "elsewhere"
        elsewhere.mkdir()
        with mock.patch.dict(os.environ, {"CODEX_HOME": str(elsewhere)}):
            self.assertEqual(R.register_codex(self.home, ROOT), "added")
        self.assertTrue((elsewhere / "config.toml").exists())
        self.assertFalse((self.home / ".codex").exists())


class Both(Home):
    def test_register_reports_each_and_the_home_is_the_given_one(self):
        (self.home / ".claude").mkdir()
        self.assertEqual(R.register(self.home, ROOT), {"claude": "added", "codex": "skipped"})
        self.assertEqual(R.register(self.home, ROOT), {"claude": "present", "codex": "skipped"})
        # with no home given it is HOME's, here the temporary one
        self.assertEqual(R.register(None, ROOT)["claude"], "present")

    def test_the_command_is_the_installed_hosts_mcp(self):
        self.assertEqual(R.command_for(Path("/home/me/.local/share/deductum")), ["python3", "/home/me/.local/share/deductum/host/deductum_host.py", "mcp"])


class AgentIds(unittest.TestCase):
    """The id the app gives a new agent (`DEDUCTUM_AGENT_ID`) is the key the host's grant names"""

    def test_the_hosts_key_for_the_apps_agent_id(self):
        with mock.patch.dict(os.environ, {"DEDUCTUM_AGENT_ID": "deductum-s42-7"}):
            ident = M.identity_from_env()
        self.assertEqual(ident, {"provider": "deductum-s42-7", "conversation": "deductum-s42-7"})
        # (the host joins them as `provider:conversation`, and a grant to that key needs no earlier visit)
        self.assertEqual(f"{ident['provider']}:{ident['conversation']}", "deductum-s42-7:deductum-s42-7")


if __name__ == "__main__":
    unittest.main()
