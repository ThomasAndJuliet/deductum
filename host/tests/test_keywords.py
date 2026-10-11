"""A line asking `claude` or `codex` for a proof: the prover sees a `sorry` of the same length"""
import os
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
import isabellum_host as h  # noqa: E402


class ProverText(unittest.TestCase):
    def test_a_keyword_line_reads_as_sorry_of_the_same_length(self):
        text = 'lemma "P"\n  claude[opus] try induction\n\nlemma "Q"\n  show ?thesis unfolding f_def codex\n'
        seen = h.prover_text(text)
        self.assertEqual(len(seen), len(text))
        self.assertIn("\n  sorry                     \n", seen)
        self.assertIn("show ?thesis unfolding f_def sorry\n", seen)

    def test_strings_and_other_words_are_left_alone(self):
        text = 'lemma "claude"\n  by simp\ntext \\<open>written by claudette\\<close>\n'
        self.assertEqual(h.prover_text(text), text)

    def test_the_model_and_hint(self):
        m = h.KEYWORD_LINES.search("  codex[astra] use the lemma foo\n")
        self.assertEqual((m.group(2), m.group(3), m.group(4)), ("codex", "astra", "use the lemma foo"))


if __name__ == "__main__":
    unittest.main()
