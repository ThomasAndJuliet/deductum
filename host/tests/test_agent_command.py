"""Configured native agent argv stays data, including prompts with shell syntax."""
import asyncio
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import deductum_host as H
import drivers


class AgentCommands(unittest.IsolatedAsyncioTestCase):
    async def test_configured_python_prefix_preserves_prompt_and_existing_agent_flags(self):
        with tempfile.TemporaryDirectory(prefix="Deductum Å agent ") as temp:
            script = Path(temp) / "agent with spaces.py"
            script.write_text("import json,sys\nprint(json.dumps(sys.argv[1:]))\n", encoding="utf-8")
            host = H.Host()
            with patch.object(H, "STATE_DIR", Path(temp) / "state"), patch.object(drivers, "read_config", return_value={
                    "agent_commands": {"claude": [sys.executable, str(script)]}}):
                session = H.Session(host, "argv", {"folder": temp, "backend": "lean", "lifecycle": "Stopped"})
                prompt = '∀😀\nquoted "text"; $literal & | < >'
                output, error = await asyncio.wait_for(session._ask_llm("claude", "fixture-model", prompt, temp, "fixture"), 10)
                self.assertEqual(error, "")
                argv = json.loads(output)
                self.assertEqual(argv[:2], ["-p", prompt])
                self.assertEqual(argv[-2:], ["--model", "fixture-model"])
                self.assertIn("--disallowedTools", argv)
                self.assertEqual(argv[argv.index("--output-format") + 1], "text")

    async def test_invalid_configured_command_is_an_actionable_failure(self):
        with tempfile.TemporaryDirectory() as temp:
            host = H.Host()
            with patch.object(H, "STATE_DIR", Path(temp) / "state"):
                session = H.Session(host, "invalid-command", {"folder": temp, "backend": "lean", "lifecycle": "Stopped"})
                for commands in (None, [], {"claude": []}, {"claude": "python"}, {"claude": [""]}, {"claude": [12]}):
                    with self.subTest(commands=commands), patch.object(drivers, "read_config", return_value={"agent_commands": commands}):
                        with self.assertRaisesRegex(H.HostError, "agent_commands"):
                            await session._ask_llm("claude", None, "fixture", temp, "fixture")


if __name__ == "__main__":
    unittest.main()
