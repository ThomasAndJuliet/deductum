#!/usr/bin/env python3
"""Check PAM rule migration without modifying the machine's PAM files."""

from pathlib import Path
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "src/agent/ct-sudo-setup.sh"
FUNCTIONS = SCRIPT.read_text().rsplit('\ncase "${1:-}" in', 1)[0]
BEGIN = "# cosmic-term sudo: begin (approvals from COSMIC Terminal; see ct-sudo-setup)"
END = "# cosmic-term sudo: end"
RULE = "auth       sufficient   /usr/local/lib/cosmic-term/pam_cosmic_term_sudo.so"
PAM = "#%PAM-1.0\nauth include system-auth\naccount include system-auth\nsession include system-auth\n"


class PamRules(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.pam = Path(self.tmp.name) / "sudo"

    def run_function(self, name):
        # Ownership and SELinux labeling are exercised in the enforcing KVM
        # guest. These fixtures isolate preservation of the PAM file text.
        harness = FUNCTIONS + '\nchown() { :; }\nrestorecon() { :; }\n' + name + ' "$1"\n'
        return subprocess.run(
            ["sh", "-c", harness, "pam-fixture", str(self.pam)],
            capture_output=True, text=True, timeout=5,
        )

    def assert_success(self, result):
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_install_preserves_password_fallback_and_original_backup(self):
        self.pam.write_text(PAM)
        self.assert_success(self.run_function("list_module"))
        self.assertEqual(self.pam.read_text(), PAM.replace("auth include", f"{BEGIN}\n{RULE}\n{END}\nauth include", 1))
        self.assertEqual(Path(str(self.pam) + ".cosmic-term-backup").read_text(), PAM)

    def test_legacy_rule_is_migrated_without_losing_later_pam_edits(self):
        existing = PAM.replace("auth include", f"{BEGIN}\nauth sufficient pam_cosmic_term_sudo.so\n{END}\nauth include", 1)
        existing += "session optional pam_custom.so\n"
        self.pam.write_text(existing)
        self.assert_success(self.run_function("list_module"))
        migrated = self.pam.read_text()
        self.assertEqual(migrated.count(BEGIN), 1)
        self.assertIn(RULE, migrated)
        self.assertIn("session optional pam_custom.so\n", migrated)
        self.assert_success(self.run_function("list_module"))
        self.assertEqual(self.pam.read_text(), migrated)

    def test_uninstall_preserves_all_unrelated_pam_rules(self):
        self.pam.write_text(PAM)
        self.assert_success(self.run_function("list_module"))
        self.assert_success(self.run_function("unlist_module"))
        self.assertEqual(self.pam.read_text(), PAM)

    def test_unfinished_blocks_are_rejected_without_replacing_the_file(self):
        broken = f"{BEGIN}\nauth sufficient pam_cosmic_term_sudo.so\n" + PAM
        for operation in ("list_module", "unlist_module"):
            with self.subTest(operation=operation):
                self.pam.write_text(broken)
                result = self.run_function(operation)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("unfinished", result.stderr)
                self.assertEqual(self.pam.read_text(), broken)
                self.assertFalse(Path(str(self.pam) + ".cosmic-term-new").exists())

    def test_install_without_an_authentication_stack_does_not_replace_it(self):
        original = "account include system-auth\n"
        self.pam.write_text(original)
        self.assertNotEqual(self.run_function("list_module").returncode, 0)
        self.assertEqual(self.pam.read_text(), original)


if __name__ == "__main__":
    unittest.main()
