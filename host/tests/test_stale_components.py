"""Stale Deductum backend registrations are found by content, not by the checkout's name."""
import os
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
os.environ.setdefault("DEDUCTUM_STATE_DIR", tempfile.mkdtemp())
from drivers.isabelle import stale_backend_components  # noqa: E402


class IdentityAdapter:
    def to_native(self, path):
        return path


def make_component(root, *parts, backend=True):
    """An Isabelle component directory; with backend, one carrying the Deductum backend's
    own etc/settings and etc/build.props markers"""
    d = Path(root, *parts)
    (d / "etc").mkdir(parents=True)
    if backend:
        (d / "etc" / "settings").write_text('DEDUCTUM_BACKEND_HOME="$COMPONENT"\n')
        (d / "etc" / "build.props").write_text("services = \\\n  deductum.Tools\n")
    else:
        (d / "etc" / "settings").write_text('OTHER_HOME="$COMPONENT"\n')
    return str(d)


class StaleComponentTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)

    def stale(self, lines, comp):
        return stale_backend_components(IdentityAdapter(), lines, comp)

    def test_checkout_outside_a_deductum_named_directory(self):
        comp = make_component(self.tmp.name, "share", "backend", "Isabelle2025")
        checkout = make_component(self.tmp.name, "work", "prover-app", "backend", "Isabelle2025")
        self.assertEqual(self.stale([checkout, comp], comp), [checkout])

    def test_installed_path(self):
        comp = make_component(self.tmp.name, "a", "backend", "Isabelle2025")
        installed = make_component(self.tmp.name, ".local", "share", "deductum", "backend", "Isabelle2025")
        self.assertEqual(self.stale([installed], comp), [installed])

    def test_other_component_is_left_alone(self):
        comp = make_component(self.tmp.name, "a", "backend", "Isabelle2025")
        other = make_component(self.tmp.name, "other", "component", backend=False)
        lookalike = make_component(self.tmp.name, "deductum-x", "backend", "Other", backend=False)
        self.assertEqual(self.stale([other, lookalike], comp), [])

    def test_current_component_is_kept(self):
        comp = make_component(self.tmp.name, "a", "backend", "Isabelle2025")
        self.assertEqual(self.stale([comp], comp), [])

    def test_missing_directory_falls_back_to_the_path_pattern(self):
        comp = make_component(self.tmp.name, "a", "backend", "Isabelle2025")
        gone = os.path.join(self.tmp.name, "deductum-old", "backend", "Isabelle2024")
        unrelated = os.path.join(self.tmp.name, "gone", "elsewhere")
        self.assertEqual(self.stale([gone, unrelated], comp), [gone])


if __name__ == "__main__":
    unittest.main()
