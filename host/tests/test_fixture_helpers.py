"""Execute synthetic commands through the same native shell boundary as tools."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from fixture_helpers import json_executable


@unittest.skipUnless(os.name == "nt", "Native Windows batch fixture encoding and expansion")
class WindowsCommandFixture(unittest.TestCase):
    def test_unicode_script_path_and_literal_percent_and_bang_survive_batch_execution(self):
        with tempfile.TemporaryDirectory(prefix="Deductum β proof 😀 ") as temporary:
            root = Path(temporary)
            scripts = root / "%PATH%!"
            scripts.mkdir()
            expected = {"marker": "β 😀", "hits": []}
            generated = json_executable(scripts, "tool", expected)
            # Keep the dispatch path free of variable syntax, while the batch
            # body must pass the actual percent/bang script path unchanged.
            command = root / "dispatch.cmd"
            command.write_bytes(generated.read_bytes())
            self.assertTrue(command.read_bytes().isascii())
            result = subprocess.run([os.environ.get("COMSPEC", "cmd.exe"), "/d", "/v:on", "/c", str(command)],
                                    capture_output=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
            self.assertEqual(json.loads(result.stdout), expected)


if __name__ == "__main__":
    unittest.main()
