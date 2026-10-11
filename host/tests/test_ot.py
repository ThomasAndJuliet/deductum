import os
import random
import sys
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import isabellum_ot as ot  # noqa: E402


def random_change(rng, length, n=3):
    points = sorted(rng.sample(range(length + 1), min(length + 1, 2 * n)))
    reps = []
    for i in range(0, len(points) - 1, 2):
        s, e = points[i], points[i + 1]
        if rng.random() < 0.3:
            e = s
        text = "".join(rng.choice("xyz") for _ in range(rng.randint(0, 3)))
        reps.append((s, e, text))
    return ot.normalize(reps, length)


def overlaps(a, b):
    return any(ot._overlap(x, y) for x in a for y in b)


def simultaneous(text, first, second):
    """Both changes applied to the same text, ``first`` before ``second`` at ties"""
    tagged = [(s, e, t, 0) for s, e, t in first] + [(s, e, t, 1) for s, e, t in second]
    tagged.sort(key=lambda r: (r[0], r[1] != r[0], r[3]))
    out, pos = [], 0
    for s, e, t, _ in tagged:
        out.append(text[pos:s])
        out.append(t)
        pos = max(pos, e)
    out.append(text[pos:])
    return "".join(out)


class TestBasics(unittest.TestCase):
    def test_apply_and_invert(self):
        text = "hello world"
        change = ot.normalize([(0, 5, "goodbye"), (6, 11, "moon")], len(text))
        new = ot.apply(text, change)
        self.assertEqual(new, "goodbye moon")
        self.assertEqual(ot.apply(new, ot.invert(text, change)), text)

    def test_normalize_merges_adjacent(self):
        self.assertEqual(ot.normalize([(0, 1, "a"), (1, 2, "b")], 3), [(0, 2, "ab")])
        with self.assertRaises(ValueError):
            ot.normalize([(0, 2, "a"), (1, 3, "b")], 3)

    def test_map_pos(self):
        change = [(2, 4, "XYZ")]
        self.assertEqual(ot.map_pos(1, change), 1)
        self.assertEqual(ot.map_pos(2, change), 2)
        self.assertEqual(ot.map_pos(2, change, right=True), 5)
        self.assertEqual(ot.map_pos(3, change), 2)
        self.assertEqual(ot.map_pos(3, change, right=True), 5)
        self.assertEqual(ot.map_pos(4, change), 5)
        self.assertEqual(ot.map_pos(6, change), 7)
        ins = [(3, 3, "ab")]
        self.assertEqual(ot.map_pos(3, ins), 3)
        self.assertEqual(ot.map_pos(3, ins, right=True), 5)

    def test_strict_conflict(self):
        with self.assertRaises(ot.Conflict):
            ot.rebase([(2, 6, "a")], [(4, 8, "b")], "strict")

    def test_strict_rebase_disjoint(self):
        text = "0123456789"
        a = [(7, 8, "A")]
        b = [(1, 3, "BBBB")]
        a2, _ = ot.rebase(a, b, "strict")
        self.assertEqual(ot.apply(ot.apply(text, b), a2), "0BBBB3456A89")

    def test_partial_drops_overlap(self):
        a = [(0, 2, "A"), (5, 6, "C")]
        b = [(1, 3, "B")]
        kept, dropped = ot.rebase_through(a, [b], "partial")
        self.assertEqual(dropped, [(0, 2, "A")])
        self.assertEqual(kept, [(4, 5, "C")])

    def test_merge_keeps_other_text(self):
        # The human deletes 2..8 while an agent replaced 4..6 with "AGENT"
        text = "0123456789"
        agent = [(4, 6, "AGENT")]
        human = [(2, 8, "h")]
        merged, _ = ot.rebase(human, agent, "merge")
        result = ot.apply(ot.apply(text, agent), merged)
        self.assertIn("AGENT", result)
        self.assertIn("h", result)
        self.assertEqual(result, "01hAGENT89")

    def test_diff(self):
        self.assertEqual(ot.diff("abcdef", "abXdef"), [(2, 3, "X")])
        self.assertEqual(ot.diff("same", "same"), [])
        self.assertEqual(ot.apply("aaa", ot.diff("aaa", "aaaa")), "aaaa")

    def test_ranges(self):
        self.assertEqual(ot.map_range(4, 8, [(4, 4, "x")], inclusive=True), (4, 9))
        self.assertEqual(ot.map_range(4, 8, [(4, 4, "x")], inclusive=False), (5, 9))
        self.assertTrue(ot.touches(4, 8, [(5, 6, "")]))
        self.assertFalse(ot.touches(4, 8, [(8, 9, "")]))

    def test_lines(self):
        text = "ab\ncd\n"
        self.assertEqual(ot.lines_cols(text, 4), (2, 1))
        self.assertEqual(ot.offset_of(text, 2, 1), 4)
        self.assertEqual(ot.offset_of(text, 9, 0), len(text))


class TestProperties(unittest.TestCase):
    def test_invert_roundtrip(self):
        rng = random.Random(1)
        for _ in range(2000):
            text = "".join(rng.choice("abc\n") for _ in range(rng.randint(0, 20)))
            change = random_change(rng, len(text))
            self.assertEqual(ot.apply(ot.apply(text, change), ot.invert(text, change)), text)

    def test_strict_matches_simultaneous(self):
        rng = random.Random(2)
        checked = 0
        for _ in range(4000):
            text = "".join(rng.choice("abcd") for _ in range(rng.randint(0, 16)))
            a = random_change(rng, len(text))
            b = random_change(rng, len(text))
            if overlaps(a, b):
                with self.assertRaises(ot.Conflict):
                    ot.rebase(a, b, "strict")
                continue
            a2, _ = ot.rebase(a, b, "strict")
            self.assertEqual(ot.apply(ot.apply(text, b), a2), simultaneous(text, b, a),
                             (text, a, b, a2))
            checked += 1
        self.assertGreater(checked, 500)

    def test_merge_never_drops_inserted_text(self):
        rng = random.Random(3)
        for _ in range(4000):
            text = "".join(rng.choice("abcd") for _ in range(rng.randint(0, 16)))
            a = random_change(rng, len(text))
            b = random_change(rng, len(text))
            a2, _ = ot.rebase(a, b, "merge")
            ot.normalize(a2, len(ot.apply(text, b)))  # valid change
            result = ot.apply(ot.apply(text, b), a2)
            for _, _, t in b:
                self.assertIn(t, result, (text, a, b, a2))
            for _, _, t in a:
                self.assertIn(t, result, (text, a, b, a2))

    def test_partial_through_several(self):
        rng = random.Random(4)
        for _ in range(2000):
            text = "".join(rng.choice("abcd") for _ in range(rng.randint(0, 16)))
            a = random_change(rng, len(text))
            laters = []
            cur = text
            for _ in range(rng.randint(1, 3)):
                c = random_change(rng, len(cur))
                laters.append(c)
                cur = ot.apply(cur, c)
            kept, dropped = ot.rebase_through(a, laters, "partial")
            ot.normalize(kept, len(cur))
            self.assertEqual(len(dropped) <= len(a), True)


if __name__ == "__main__":
    unittest.main()
