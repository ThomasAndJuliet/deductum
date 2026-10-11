"""Text changes for the Isabellum session host: applying, inverting and rebasing.

A *change* is a list of replacements ``(start, end, text)`` against one revision of
a document, sorted by ``start`` and not overlapping. Offsets count code points.

Rebasing moves a change made against an older revision past the changes committed
since. Three policies differ only in what they do where the changes overlap:

* ``strict`` (agents' edits, design C7): overlap is a conflict, reported, nothing applied;
* ``partial`` (undo and revert, C7): the parts that overlap are left out and returned,
  so the caller can keep them as recoverable alternatives;
* ``merge`` (the human's typing, C11): nothing is dropped; a deletion that ran over
  text someone else has since replaced deletes only the text that is still there.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Iterable, List, Optional, Sequence, Tuple

Replacement = Tuple[int, int, str]
Change = List[Replacement]


class Conflict(Exception):
    """Replacements of a change overlap text changed since its base revision"""

    def __init__(self, overlaps: List[Tuple[Replacement, Replacement]]):
        super().__init__("overlapping changes")
        self.overlaps = overlaps


def normalize(change: Iterable[Sequence], length: int) -> Change:
    """Sorted, validated, with adjacent and empty replacements merged or dropped"""
    reps = sorted(((int(s), int(e), str(t)) for s, e, t in change), key=lambda r: (r[0], r[1]))
    out: Change = []
    for s, e, t in reps:
        if s < 0 or e < s or e > length:
            raise ValueError(f"replacement {s}..{e} outside 0..{length}")
        if s == e and not t:
            continue
        if out and s < out[-1][1]:
            raise ValueError(f"overlapping replacements at {s}")
        if out and s == out[-1][1]:
            ps, pe, pt = out[-1]
            out[-1] = (ps, e, pt + t)
        else:
            out.append((s, e, t))
    return out


def apply(text: str, change: Change) -> str:
    parts = []
    pos = 0
    for s, e, t in change:
        parts.append(text[pos:s])
        parts.append(t)
        pos = e
    parts.append(text[pos:])
    return "".join(parts)


def invert(text: str, change: Change) -> Change:
    """The change that undoes ``change``, against the text it produces"""
    out: Change = []
    delta = 0
    for s, e, t in change:
        ns = s + delta
        out.append((ns, ns + len(t), text[s:e]))
        delta += len(t) - (e - s)
    return out


def delta_before(change: Change, pos: int) -> int:
    return sum(len(t) - (e - s) for s, e, t in change if e <= pos)


def map_pos(pos: int, change: Change, right: bool = False) -> int:
    """Where ``pos`` lies after ``change``. Inside a replaced range it moves to the
    range's start, or with ``right`` past its new text; at an insertion point, or the
    start of a replaced range, ``right`` puts it after the new text."""
    delta = 0
    for s, e, t in change:
        if s == e:
            if pos > s or (pos == s and right):
                delta += len(t)
                continue
            break
        if pos >= e:
            delta += len(t) - (e - s)
            continue
        if pos < s or (pos == s and not right):
            break
        return s + delta + (len(t) if right else 0)
    return pos + delta


def map_range(start: int, end: int, change: Change, inclusive: bool) -> Tuple[int, int]:
    """A tracked range after ``change``. ``inclusive`` ranges grow by insertions at their
    ends (a granted proof); exclusive ones don't (an agent's inserted text)."""
    if inclusive:
        ns, ne = map_pos(start, change, right=False), map_pos(end, change, right=True)
    else:
        ns, ne = map_pos(start, change, right=True), map_pos(end, change, right=False)
    return ns, max(ns, ne)


def touches(start: int, end: int, change: Change) -> bool:
    """Whether ``change`` alters text inside ``start..end`` or inserts strictly inside it"""
    for s, e, t in change:
        if max(s, start) < min(e, end):
            return True
        if s == e and start < s < end:
            return True
        if start == end and s < start < e:
            return True
    return False


def _overlap(a: Replacement, b: Replacement) -> bool:
    (s, e, _), (bs, be, _) = a, b
    if max(s, bs) < min(e, be):
        return True
    if s == e and bs < s < be:
        return True
    if bs == be and s < bs < e:
        return True
    return False


def _merge(reps: Change) -> Change:
    out: Change = []
    for s, e, t in sorted(reps, key=lambda r: (r[0], r[1])):
        if out and s <= out[-1][1]:
            ps, pe, pt = out[-1]
            out[-1] = (ps, max(pe, e), pt + t)
        else:
            out.append((s, e, t))
    return [r for r in out if r[0] != r[1] or r[2]]


def _insertion_point(pos: int, change: Change) -> int:
    """Where an insertion at ``pos`` goes: after insertions made at the same point
    (they were committed first), before a replacement starting there"""
    delta = 0
    for s, e, t in change:
        if s == e:
            if pos >= s:
                delta += len(t)
                continue
            break
        if pos >= e:
            delta += len(t) - (e - s)
            continue
        if pos <= s:
            break
        return s + delta
    return pos + delta


def _move(a: Replacement, later: Change) -> Replacement:
    s, e, t = a
    if s == e:
        ns = _insertion_point(s, later)
        return (ns, ns, t)
    ns = map_pos(s, later, right=True)
    ne = ns if s == e else max(ns, map_pos(e, later, right=False))
    return (ns, ne, t)


def _merge_one(a: Replacement, hits: Change, later: Change) -> Change:
    """``a`` rebased past overlapping ``hits`` without dropping anyone's text"""
    s, e, t = a
    segments = []
    cur = s
    for bs, be, _ in sorted(hits):
        if bs > cur:
            segments.append((cur, min(bs, e)))
        cur = max(cur, be)
    if cur < e:
        segments.append((cur, e))
    inside = any(bs < s < be for bs, be, _ in hits)
    ins = map_pos(s, later, right=inside)
    out: Change = [(ins, ins, t)] if t else []
    for ss, ee in segments:
        if ee > ss:
            ns = map_pos(ss, later, right=True)
            out.append((ns, max(ns, map_pos(ee, later, right=False)), ""))
    return out


def rebase(change: Change, later: Change, policy: str = "strict") -> Tuple[Change, Change]:
    """Move ``change`` past ``later``, both against the same revision.

    Returns the rebased change and, for the ``partial`` policy, the replacements left
    out (against the *old* revision). ``strict`` raises :class:`Conflict` instead.
    For ``strict`` and ``partial`` the kept replacements stay in order, one for one."""
    out: Change = []
    dropped: Change = []
    overlaps = []
    for a in change:
        hits = [b for b in later if _overlap(a, b)]
        if not hits:
            out.append(_move(a, later))
        elif policy == "strict":
            overlaps.extend((a, b) for b in hits)
        elif policy == "partial":
            dropped.append(a)
        else:
            out.extend(_merge_one(a, hits, later))
    if overlaps:
        raise Conflict(overlaps)
    if policy == "merge":
        return _merge(out), dropped
    return out, dropped


def rebase_through(change: Change, laters: Iterable[Change], policy: str = "strict"
                   ) -> Tuple[Change, Change]:
    """Rebase through several later changes in order. Parts dropped by ``partial`` are
    reported against the original revision."""
    current = list(change)
    origin = list(change)
    dropped_all: Change = []
    for later in laters:
        if policy == "partial":
            kept, kept_origin = [], []
            for rep, orig in zip(current, origin):
                if any(_overlap(rep, b) for b in later):
                    dropped_all.append(orig)
                else:
                    kept.append(_move(rep, later))
                    kept_origin.append(orig)
            current, origin = kept, kept_origin
        else:
            current, _ = rebase(current, later, policy)
            origin = current
    if policy == "partial":
        current = _merge(current)
    return current, dropped_all


def fine(old: str, new: str) -> Change:
    """The replacements turning ``old`` into ``new``, character by character between
    their common prefix and suffix"""
    import difflib
    if old == new:
        return []
    n = min(len(old), len(new))
    p = 0
    while p < n and old[p] == new[p]:
        p += 1
    q = 0
    while q < n - p and old[len(old) - 1 - q] == new[len(new) - 1 - q]:
        q += 1
    a, b = old[p:len(old) - q], new[p:len(new) - q]
    if len(a) * len(b) > 4_000_000:
        return [(p, len(old) - q, b)]
    out: Change = []
    for tag, i1, i2, j1, j2 in difflib.SequenceMatcher(None, a, b, autojunk=False).get_opcodes():
        if tag != "equal":
            out.append((p + i1, p + i2, b[j1:j2]))
    return out


def diff(old: str, new: str) -> Change:
    """One replacement turning ``old`` into ``new``: the span between common affixes"""
    if old == new:
        return []
    n = min(len(old), len(new))
    p = 0
    while p < n and old[p] == new[p]:
        p += 1
    q = 0
    while q < n - p and old[len(old) - 1 - q] == new[len(new) - 1 - q]:
        q += 1
    return [(p, len(old) - q, new[p:len(new) - q])]


def sequential(change: Change) -> List[Replacement]:
    """The replacements in an order where each one's offsets are valid after the
    previous ones are applied: from the end of the text backwards"""
    return sorted(change, key=lambda r: r[0], reverse=True)


@dataclass
class Span:
    start: int
    end: int

    def mapped(self, change: Change, inclusive: bool) -> "Span":
        s, e = map_range(self.start, self.end, change, inclusive)
        return Span(s, e)


def lines_cols(text: str, offset: int) -> Tuple[int, int]:
    """1-based line and 0-based column (code points) of ``offset``"""
    line = text.count("\n", 0, offset) + 1
    col = offset - (text.rfind("\n", 0, offset) + 1)
    return line, col


def offset_of(text: str, line: int, col: int) -> int:
    """Offset of 1-based ``line`` and 0-based ``col``, clamped to the text"""
    pos = 0
    for _ in range(max(0, line - 1)):
        nxt = text.find("\n", pos)
        if nxt < 0:
            return len(text)
        pos = nxt + 1
    end = text.find("\n", pos)
    end = len(text) if end < 0 else end
    return min(pos + max(0, col), end)
