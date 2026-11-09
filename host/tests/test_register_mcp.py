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


def _stale_entry(command, args=None):
    return {"type": "stdio", "command": command, "args": args or ["/opt/deductum/host/deductum_host.py", "mcp"], "env": {}}


class Command(Home):
    def test_macos_uses_the_hosts_own_python(self):
        with mock.patch.object(sys, "platform", "darwin"), mock.patch.object(sys, "executable", "/App/python/arm64/bin/python3"):
            self.assertEqual(R.command_for(ROOT), ["/App/python/arm64/bin/python3", "/opt/deductum/host/deductum_host.py", "mcp"])
            (self.home / ".claude").mkdir()
            self.assertEqual(R.register_claude(self.home, ROOT), "added")
            self.assertEqual(self.claude()["mcpServers"]["deductum"]["command"], "/App/python/arm64/bin/python3")

    def test_linux_stays_python3(self):
        with mock.patch.object(sys, "platform", "linux"), mock.patch.object(sys, "executable", "/usr/bin/python3.12"):
            self.assertEqual(R.command_for(ROOT)[0], "python3")

    def _mac(self):
        for p in (mock.patch.object(sys, "platform", "darwin"), mock.patch.object(sys, "executable", sys.executable)):
            p.start()
            self.addCleanup(p.stop)


class ClaudeRepair(Command):
    def write(self, entry, extra=None):
        data = {"numStartups": 1, "mcpServers": {"other": {"command": "o"}, "deductum": entry}}
        (self.home / ".claude.json").write_text(json.dumps(data, indent=2))
        return data

    def test_a_stale_absolute_path_is_replaced_in_command_only(self):
        self._mac()
        entry = _stale_entry("/gone/python3")
        entry["env"] = {"K": "v"}
        before = self.write(entry)
        self.assertEqual(R.register_claude(self.home, ROOT), "updated")
        after = self.claude()
        before["mcpServers"]["deductum"]["command"] = sys.executable
        self.assertEqual(after, before)

    def test_a_bare_name_that_is_not_on_path_is_replaced(self):
        self._mac()
        self.write(_stale_entry("python3"))
        with mock.patch("shutil.which", return_value=None):
            self.assertEqual(R.register_claude(self.home, ROOT), "updated")
        self.assertEqual(self.claude()["mcpServers"]["deductum"]["command"], sys.executable)

    def test_a_working_command_is_left_alone(self):
        for cmd in (sys.executable, "python3"):
            self.write(_stale_entry(cmd))
            text = (self.home / ".claude.json").read_text()
            with mock.patch("shutil.which", return_value="/usr/bin/python3"):
                self.assertEqual(R.register_claude(self.home, ROOT), "present")
            self.assertEqual((self.home / ".claude.json").read_text(), text)

    def test_someone_elses_entry_or_other_args_are_left_alone(self):
        for entry in (_stale_entry("/gone/python3", ["/home/me/dev/deductum_host.py", "mcp"]), _stale_entry("/gone/python3", ["/opt/deductum/host/deductum_host.py"]), {"command": "/gone/x"}, "odd"):
            self.write(entry)
            text = (self.home / ".claude.json").read_text()
            self.assertEqual(R.register_claude(self.home, ROOT), "present")
            self.assertEqual((self.home / ".claude.json").read_text(), text)


class CodexRepair(Command):
    def put(self, text):
        (self.home / ".codex").mkdir(exist_ok=True)
        (self.home / ".codex" / "config.toml").write_text(text)

    def toml(self):
        return (self.home / ".codex" / "config.toml").read_text()

    def test_only_the_command_line_of_our_table_changes(self):
        self._mac()
        before = 'model = "x"\ncommand = "keep"\n\n[mcp_servers.deductum]\ncommand = "/gone/python3"  # mine\nargs = ["/opt/deductum/host/deductum_host.py", "mcp"]\n\n[mcp_servers.deductum.env]\nA = "1"\n\n[other]\ncommand = "/gone/z"\n'
        self.put(before)
        self.assertEqual(R.register_codex(self.home, ROOT), "updated")
        self.assertEqual(self.toml(), before.replace('"/gone/python3"', json.dumps(sys.executable)))
        self.assertEqual(R.register_codex(self.home, ROOT), "present")

    def test_bare_python3_missing_is_replaced_and_crlf_kept(self):
        self._mac()
        self.put('[mcp_servers.deductum]\r\ncommand = "python3"\r\nargs = ["/opt/deductum/host/deductum_host.py", "mcp"]\r\n')
        with mock.patch("shutil.which", return_value=None):
            self.assertEqual(R.register_codex(self.home, ROOT), "updated")
        with open(self.home / ".codex" / "config.toml", newline="") as f:
            self.assertEqual(f.read(), '[mcp_servers.deductum]\r\ncommand = %s\r\nargs = ["/opt/deductum/host/deductum_host.py", "mcp"]\r\n' % json.dumps(sys.executable))

    def test_working_foreign_or_odd_tables_are_left_alone(self):
        self._mac()
        for text in (
            '[mcp_servers.deductum]\ncommand = "%s"\nargs = ["/opt/deductum/host/deductum_host.py", "mcp"]\n' % sys.executable,
            '[mcp_servers.deductum]\ncommand = "/gone/p"\nargs = ["/home/me/deductum_host.py", "mcp"]\n',
            '[mcp_servers.deductum]\ncommand = "/gone/p"\nargs = [\n  "/opt/deductum/host/deductum_host.py", "mcp"]\n',
            '[mcp_servers.deductum]\ncommand = \'/gone/p\'\nargs = ["/opt/deductum/host/deductum_host.py", "mcp"]\n',
            '[mcp_servers.deductum]\ncommand = "/gone/p"\n',
        ):
            self.put(text)
            self.assertEqual(R.register_codex(self.home, ROOT), "present")
            self.assertEqual(self.toml(), text)


class Both(Home):
    def test_register_reports_each_and_the_home_is_the_given_one(self):
        (self.home / ".claude").mkdir()
        self.assertEqual(R.register(self.home, ROOT), {"claude": "added", "codex": "skipped"})
        self.assertEqual(R.register(self.home, ROOT), {"claude": "present", "codex": "skipped"})
        # with no home given it is HOME's, here the temporary one
        self.assertEqual(R.register(None, ROOT)["claude"], "present")

    def test_the_command_is_the_installed_hosts_mcp(self):
        with mock.patch.object(sys, "platform", "linux"):
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
