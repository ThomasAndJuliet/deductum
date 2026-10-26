"""The Lean 4 driver: answers the backend protocol (see ``__init__``) by talking LSP to
``lake serve`` (design: docs/deductum-provers.md §4, §5, §7.3, §9).

Lean has no command spans over LSP, so the commands of a document are found by a
lexical split (``Splitter``) that follows how Isabelle's commands tile a theory: each
top-level command, the header of a declaration (up to its ``by``), and every tactic of
a ``by`` block. Their statuses come from ``$/lean/fileProgress`` and the diagnostics.

Protocol facts this relies on, checked on Lean v4.19.0 (see tests/integration_lean.py):
``$/lean/fileProgress`` carries tactic-level ranges, the last one running to the end of
the file; ``publishDiagnostics`` is cumulative and carries the document version;
``$/lean/plainGoal`` answers with the state after a tactic when the position is past its
first character; ``Lean.Widget.getInteractiveGoals`` goes through ``$/lean/rpc/call``;
"Try this" suggestions are information diagnostics (and quickfix code actions); a changed
dependency shows as a sticky information diagnostic, not as a client notification.
"""

from __future__ import annotations

import asyncio
import bisect
import contextlib
import json
import os
import re
import shutil
import sys
import time
import subprocess
import urllib.error
import urllib.parse
import urllib.request
import uuid
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, Tuple

from . import Driver, HostError, message, resource_limit
from .lsp import LspClient, LspClosed, LspError, TextIndex, UNHANDLED

HOME = os.path.expanduser("~")
ELAN_BIN = os.path.join(HOME, ".elan", "bin")

# ---------------------------------------------------------------------------
# Keywords and their Isabelle kinds (the `syntax` operation)

GOAL_KEYWORDS = ("theorem", "lemma", "example", "corollary", "proposition")
DECL_KEYWORDS = ("def", "abbrev", "instance", "structure", "inductive", "class", "axiom", "opaque",
                 "namespace", "section", "end", "open", "variable", "universe", "set_option", "import",
                 "attribute", "notation", "infix", "infixl", "infixr", "prefix", "postfix", "macro",
                 "macro_rules", "syntax", "elab", "elab_rules", "declare_syntax_cat", "deriving", "mutual",
                 "noncomputable", "private", "protected", "unsafe", "partial", "nonrec", "export",
                 "initialize", "builtin_initialize", "local", "scoped", "include", "omit", "alias",
                 "run_cmd", "universes", "set_library_suggestions")
DIAG_KEYWORDS = ("#check", "#eval", "#print", "#reduce", "#synth", "#help", "#guard", "#guard_msgs",
                 "#check_failure", "#where", "#exit", "#find", "#simp", "#norm_num", "#lint")
TACTIC_NAMES = ("intro", "intros", "rintro", "exact", "exact?", "apply", "apply?", "refine", "refine'",
                "rw", "rw?", "rewrite", "rwa", "simp", "simp?", "simp_all", "simp_all?", "simpa", "simp_arith",
                "dsimp", "unfold", "have", "show", "calc", "cases", "rcases", "obtain", "induction", "constructor",
                "left", "right", "exists", "use", "ext", "funext", "congr", "omega", "decide", "norm_num",
                "linarith", "nlinarith", "positivity", "ring", "field_simp", "trivial", "rfl", "sorry", "assumption",
                "contradiction", "exfalso", "by_cases", "by_contra", "push_neg", "specialize", "generalize",
                "subst", "revert", "clear", "rename_i", "next", "case", "·", "|", "_", "aesop", "aesop?", "hint",
                "grind", "split", "injection", "exact_mod_cast", "norm_cast", "push_cast", "conv", "change",
                "set", "let", "suffices", "from", "all_goals", "any_goals", "first", "try", "repeat", "skip",
                "done", "fail", "admit", "trans", "symm", "calc_step", "bv_decide", "ac_rfl", "native_decide")
LEAN_KINDS: Dict[str, str] = {}
for _k in GOAL_KEYWORDS:
    LEAN_KINDS[_k] = "thy_goal_stmt"
for _k in DECL_KEYWORDS:
    LEAN_KINDS[_k] = "thy_decl"
for _k in DIAG_KEYWORDS:
    LEAN_KINDS[_k] = "diag"
for _k in TACTIC_NAMES:
    LEAN_KINDS.setdefault(_k, "prf_script")
LEAN_KINDS["/-!"] = "document_body"
LEAN_KINDS["@["] = "thy_decl"

#: Modifiers that go in front of a declaration keyword
MODIFIERS = ("private", "protected", "noncomputable", "unsafe", "partial", "nonrec", "local", "scoped")
#: Commands that can take `... in` and then another command
IN_PREFIXES = ("open", "set_option", "attribute", "include", "omit", "universe", "variable", "export")
#: Declarations whose `:= by` starts a tactic block of their own
PROOF_DECLS = ("theorem", "lemma", "example", "corollary", "proposition", "def", "abbrev", "instance", "opaque")
#: Words at column 0 that go on with the command above instead of starting one
CONTINUATIONS = ("where", "termination_by", "decreasing_by", "then", "else", "with", "from", "at", "using",
                 "only", "if", "match", "fun", "do", "let", "have", "show")
#: Tools that are tactics typed in the text (their output is a finding)
TOOL_TACTICS = ("exact?", "apply?", "rw?", "simp?", "simp_all?", "aesop?", "hint", "grind", "decide", "omega",
                "plausible", "bv_decide")

# ---------------------------------------------------------------------------
# Lexical structure: comments, strings and the split into commands

CODE, COMMENT, STRING, DOC, MODDOC = 0, 1, 2, 3, 4

_REGION_START = re.compile(r"--|/-|\"|\br#*\"|'")
_BLOCK_EDGE = re.compile(r"/-|-/")
_CHAR_LIT = re.compile(r"'(?:\\(?:x[0-9a-fA-F]{2}|u\{?[0-9a-fA-F]+\}?|.)|[^\\'\n])'")
_WORD = re.compile(r"\w[\w'?!]*(?:\.\w[\w'?!]*)*")
_SYMBOL = re.compile(r":=|=>|<;>|[^\w\s]")
OPENERS = "([{⟨⦃⟦"
CLOSERS = ")]}⟩⦄⟧"
_OPENS_BLOCK = re.compile(r"(?:\bby|=>|\bwith|\bdo|\bcalc)$")


def scan_regions(text: str) -> bytearray:
    """For each character: code, comment, string, docstring or module doc. Block comments
    nest; a quote after an identifier character is a prime, not a character literal."""
    mask = bytearray(len(text))
    n = len(text)
    pos = 0
    while True:
        m = _REGION_START.search(text, pos)
        if not m:
            break
        i, tok = m.start(), m.group()
        if tok == "--":
            j = text.find("\n", i)
            j = n if j < 0 else j
            mask[i:j] = b"\x01" * (j - i)
        elif tok == "/-":
            kind = DOC if text.startswith("/--", i) and not text.startswith("/--/", i) else \
                MODDOC if text.startswith("/-!", i) else COMMENT
            depth, k = 1, i + 2
            while depth:
                e = _BLOCK_EDGE.search(text, k)
                if not e:
                    k = n
                    break
                depth += 1 if e.group() == "/-" else -1
                k = e.end()
            j = k
            mask[i:j] = bytes([kind]) * (j - i)
        elif tok == "'":
            lit = _CHAR_LIT.match(text, i)
            prev = text[i - 1] if i else " "
            if lit and not (prev.isalnum() or prev in "_'"):
                j = lit.end()
                mask[i:j] = b"\x02" * (j - i)
            else:
                j = i + 1
        else:  # a string, plain or raw
            if tok == '"':
                k = i + 1
                while k < n and text[k] != '"':
                    k += 2 if text[k] == "\\" else 1
                j = min(k + 1, n)
            else:
                hashes = tok.count("#")
                end = text.find('"' + "#" * hashes, i + len(tok))
                j = n if end < 0 else end + 1 + hashes
            mask[i:j] = b"\x02" * (j - i)
        pos = max(j, i + 1)
    return mask


class Splitter:
    """The commands of a Lean text: ``commands`` is ``[(start, end, name)]``, non-overlapping and
    in order; ``toplevel`` the same for whole top-level commands (a declaration with its proof)."""

    def __init__(self, text: str):
        self.text = text
        self.mask = scan_regions(text)
        self.index = TextIndex(text)
        self.lines = self._lines()
        self.toplevel: List[Tuple[int, int, str]] = []
        self.commands: List[Tuple[int, int, str]] = []
        self._split()

    # ----- tokens

    def _lines(self) -> List[Tuple[int, int, int]]:
        """Per line: start, end (before the newline) and where its code starts (-1: none)"""
        out = []
        t, mask = self.text, self.mask
        starts = self.index.starts
        for i, ls in enumerate(starts):
            le = self.index.line_end(i)
            p = ls
            while p < le:
                if t[p] in " \t\r":
                    p += 1
                elif mask[p] == COMMENT:
                    p += 1
                else:
                    break
            out.append((ls, le, p if p < le else -1))
        return out

    def trim_end(self, limit: int, floor: int = 0) -> int:
        """The end of the code before ``limit``: trailing space and comments left off"""
        t, mask = self.text, self.mask
        p = limit
        while p > floor and (t[p - 1].isspace() or mask[p - 1] == COMMENT):
            p -= 1
        return p

    def tokens(self, start: int, end: int):
        """``(start, end, text, depth)`` of the code tokens in a range; depth counts the brackets
        open at the token (a closing bracket has the depth after it)"""
        t, mask = self.text, self.mask
        p, depth = start, 0
        while p < end:
            c = t[p]
            if c.isspace() or mask[p] == COMMENT:
                p += 1
                continue
            if mask[p] in (STRING, DOC, MODDOC):
                q = p
                while q < end and mask[q] == mask[p]:
                    q += 1
                yield p, q, t[p:q], depth
                p = q
                continue
            m = _WORD.match(t, p)
            if m:
                q = min(m.end(), end)
                yield p, q, t[p:q], depth
                p = q
                continue
            m = _SYMBOL.match(t, p)
            q = min(m.end(), end) if m else p + 1
            tok = t[p:q]
            if tok in OPENERS:
                yield p, q, tok, depth
                depth += 1
            elif tok in CLOSERS:
                depth = max(0, depth - 1)
                yield p, q, tok, depth
            else:
                yield p, q, tok, depth
            p = q

    # ----- top-level commands

    def _starts_command(self, p: int) -> bool:
        t = self.text
        if self.mask[p] in (DOC, MODDOC):
            return t.startswith(("/--", "/-!"), p)
        if self.mask[p] != CODE:
            return False
        if t.startswith(("@[", "#"), p):
            return True
        m = _WORD.match(t, p)
        if not m:
            return False
        word = m.group()
        if word in CONTINUATIONS:
            return False
        if word == "deriving":
            return t[m.end():].lstrip().startswith("instance")
        return True

    def _skip_prefix(self, start: int, end: int) -> Tuple[int, bool]:
        """Where a command's own keyword is, after docstrings, attributes, modifiers and
        ``open … in``; and whether only such prefixes were found (the command is not done)"""
        toks = list(self.tokens(start, end))
        i = 0
        while i < len(toks):
            s, e, tok, _ = toks[i]
            if self.mask[s] == DOC:
                i += 1
            elif tok == "@" and i + 1 < len(toks) and toks[i + 1][2] == "[":
                depth_at = toks[i + 1][3]
                i += 2
                while i < len(toks) and not (toks[i][2] == "]" and toks[i][3] == depth_at):
                    i += 1
                i += 1
            elif tok in MODIFIERS and i + 1 < len(toks) and toks[i + 1][2] not in (":", ":="):
                i += 1
            elif tok in IN_PREFIXES:
                j = next((k for k in range(i, len(toks)) if toks[k][2] == "in" and toks[k][3] == 0), None)
                if j is None:
                    break
                i = j + 1
                if i >= len(toks):
                    return end, True
            else:
                break
        if i >= len(toks):
            return end, True
        return toks[i][0], False

    def _name_at(self, p: int) -> str:
        t = self.text
        if t.startswith("/-!", p):
            return "/-!"
        if t.startswith("@[", p):
            return "@["
        if t[p] == "#":
            m = re.compile(r"#\w+").match(t, p)
            return m.group() if m else "#"
        m = _WORD.match(t, p)
        return m.group() if m else t[p]

    def _split(self) -> None:
        t, mask = self.text, self.mask
        groups: List[List[int]] = []  # [start, limit]
        for ls, le, fp in self.lines:
            if fp < 0:
                continue
            col0 = fp == ls
            if groups and groups[-1][2] == 1 and fp < groups[-1][1]:
                continue  # inside a module doc
            if col0 and mask[fp] == MODDOC and t.startswith("/-!", fp):
                j = fp
                while j < len(t) and mask[j] == MODDOC:
                    j += 1
                groups.append([fp, j, 1])
                continue
            if col0 and self._starts_command(fp) and not (groups and self._unfinished(groups[-1])):
                groups.append([fp, le, 0])
            elif groups:
                groups[-1][1] = max(groups[-1][1], le)
            else:
                groups.append([fp, le, 0])
        for start, limit, moddoc in groups:
            end = self.trim_end(limit, start) if not moddoc else limit
            if moddoc:
                self.toplevel.append((start, end, "/-!"))
                self.commands.append((start, end, "/-!"))
                continue
            kw, _ = self._skip_prefix(start, end)
            name = self._name_at(kw) if kw < end else self._name_at(start)
            self.toplevel.append((start, end, name))
            self._split_command(start, end, kw, name)

    def _unfinished(self, group: List[int]) -> bool:
        """Is the command so far only a prefix (docstring, attributes, ``open … in``)?"""
        start, limit = group[0], group[1]
        end = self.trim_end(limit, start)
        return self._skip_prefix(start, end)[1]

    # ----- inside a declaration

    def _split_command(self, start: int, end: int, kw: int, name: str) -> None:
        if name in PROOF_DECLS:
            toks = list(self.tokens(kw, end))
            by = next((tk for tk in toks if tk[2] == "by" and tk[3] == 0), None)
            if by:
                self.commands.append((start, by[1], name))
                self._tactics(by[1], end)
                return
            if len(toks) >= 2 and toks[-1][2] == "sorry" and toks[-2][2] == ":=" and toks[-1][3] == 0:
                self.commands.append((start, toks[-2][1], name))
                self.commands.append((toks[-1][0], end, "sorry"))
                return
        self.commands.append((start, end, name))

    def _tactics(self, rs: int, re_: int) -> None:
        """The tactics of a ``by`` block that runs from ``rs`` to ``re_``"""
        items: List[List[int]] = []  # [start, limit, column of a bullet's block]
        stack: List[int] = []
        lo = bisect.bisect_right(self.index.starts, rs) - 1
        for ls, le, fp in self.lines[lo:]:
            if ls >= re_:
                break
            if fp < 0 or fp >= re_:
                continue
            if fp < rs:
                # code before the block starts on this line: look past it
                fp = self._first_code(rs, le)
                if fp < 0:
                    continue
            k = fp - ls
            while stack and k < stack[-1]:
                stack.pop()
            new = False
            if not stack or k == stack[-1]:
                new = True
                if not stack:
                    stack.append(k)
            elif items and (self._opens_block(items[-1]) or items[-1][2] == k):
                new = True
                stack.append(k)
            elif self.text[fp] == "_" and ":=" in self.text[fp:le]:
                new = True
                stack.append(k)
            if new:
                items.append([fp, le, self._bullet_column(fp, le, ls)])
            elif items:
                items[-1][1] = le
        for start, limit, _ in items:
            end = min(self.trim_end(limit, start), re_)
            if end > start:
                self._split_item(start, end)

    def _bullet_column(self, fp: int, le: int, ls: int) -> int:
        """The column the block of a bullet starts at (the first token after it on its line), or -1"""
        if self._tactic_name(fp) != "·":
            return -1
        q = self._first_code(fp + 1, le)
        return q - ls if q >= 0 else -1

    def _first_code(self, p: int, le: int) -> int:
        t, mask = self.text, self.mask
        while p < le and (t[p].isspace() or mask[p] == COMMENT):
            p += 1
        return p if p < le else -1

    def _opens_block(self, item: List[int]) -> bool:
        """Does the tactic end where a block of tactics begins (``by``, ``with``, the ``=>`` of a case)?"""
        end = self.trim_end(item[1], item[0])
        ls = self.index.starts[self.index.line_of(max(item[0], end - 1))]
        seg = self.text[max(ls, item[0]):end]
        if seg.strip() in ("·", "."):
            return True
        m = _OPENS_BLOCK.search(seg)
        if not m:
            return False
        if m.group() == "=>" and self._tactic_name(item[0]) not in ("|", "case", "next", "case'"):
            return False  # (the arrow of a function)
        depth = 0
        for _s, _e, tok, _d in self.tokens(item[0], end):
            depth += 1 if tok in OPENERS else -1 if tok in CLOSERS else 0
        return depth <= 0

    def _tactic_name(self, p: int) -> str:
        t = self.text
        if t[p] in "·" or (t[p] == "." and t[p + 1:p + 2] in (" ", "\n", "")):
            return "·"
        m = _WORD.match(t, p)
        if m:
            return m.group()
        return t[p]

    def _split_item(self, s: int, e: int) -> None:
        """One tactic (with its continuation lines), cut where a bullet, a ``by`` or the ``=>`` of a
        case ends a command and another starts on the same line"""
        pos = s
        while pos < e:
            name = self._tactic_name(pos)
            if name == "·":
                self.commands.append((pos, pos + 1, "·"))
                pos = self._first_code(pos + 1, e)
                if pos < 0:
                    return
                continue
            arrow = name in ("|", "case", "next", "case'")
            cut = None
            for _ts, te, tok, depth in self.tokens(pos, e):
                if depth == 0 and (tok == "by" or (arrow and tok == "=>")):
                    cut = te
                    break
            if cut is None:
                self.commands.append((pos, e, name))
                return
            self.commands.append((pos, cut, name))
            pos = self._first_code(cut, e)
            if pos < 0:
                return

    def _line_end(self, p: int) -> int:
        return self.index.line_end(self.index.line_of(p))


def split_commands(text: str) -> List[Tuple[int, int, str]]:
    return Splitter(text).commands


# ---------------------------------------------------------------------------
# Positions between versions of a text


def common_prefix_len(a: str, b: str) -> int:
    lo, hi = 0, min(len(a), len(b))
    while lo < hi:
        mid = (lo + hi + 1) // 2
        if a[:mid] == b[:mid]:
            lo = mid
        else:
            hi = mid - 1
    return lo


class TextMap:
    """Positions of an older text in a newer one, for the one changed region between them
    (found as the common prefix and suffix); positions inside the region map to None"""

    def __init__(self, old: str, new: str):
        self.same = old == new
        p = common_prefix_len(old, new)
        rest_old, rest_new = old[p:], new[p:]
        q = common_prefix_len(rest_old[::-1], rest_new[::-1])
        self.p = p
        self.old_end = len(old) - q
        self.shift = len(new) - len(old)

    def pos(self, x: int, right: bool = False) -> Optional[int]:
        if self.same or x <= self.p:
            return x
        if x >= self.old_end:
            return x + self.shift
        return None

    def span(self, s: int, e: int) -> Optional[Tuple[int, int]]:
        a, b = self.pos(s), self.pos(e, True)
        return None if a is None or b is None else (a, b)


# ---------------------------------------------------------------------------
# Reports: statuses, messages and colours of a document


STALE_MESSAGE = "Imports are out of date and should be rebuilt"
STALE_HEADER_ERROR = "Imports changed; rebuild imports"
KINDS_BY_SEVERITY = {1: "error", 2: "warning", 3: "information", 4: "writeln"}


def try_this(text: str) -> List[str]:
    """The proofs a "Try this" message offers, for Insert: the text after the colon, without the
    ``--`` lines that show the goal it leads to"""
    m = re.match(r"\s*Try (this|these):\s*(.*)$", text, re.S)
    if not m:
        return []
    lines = [ln for ln in m.group(2).split("\n") if not ln.lstrip().startswith("--")]
    if m.group(1) == "this":
        body = "\n".join(lines).strip()
        return [body] if body else []
    out = []
    for ln in lines:
        ln = re.sub(r"^\s*(?:[•*-]\s*)?(?:\[\w+\]\s*)?", "", ln).strip()
        if ln:
            out.append(ln)
    return out


def diag_message(d: Dict[str, Any]) -> Dict[str, Any]:
    """A Message for a diagnostic whose range is already in offsets"""
    text = d["message"]
    m = message(KINDS_BY_SEVERITY.get(d.get("severity", 1), "error"), text, start=d["start"], end=d["end"])
    m["sendbacks"] = try_this(text)
    return m


def anchor_command(commands: List[Tuple[int, int, str]], pos: int) -> Optional[int]:
    """The command a position belongs to: the one that contains it, else the next one"""
    if not commands:
        return None
    starts = [c[0] for c in commands]
    i = bisect.bisect_right(starts, pos) - 1
    if i >= 0 and pos <= commands[i][1]:
        return i
    return min(i + 1, len(commands) - 1)


def build_report(commands: List[Tuple[int, int, str]], diags: List[Dict[str, Any]],
                 processing: Optional[List[Tuple[int, int]]]) -> Dict[str, Any]:
    """Statuses of the commands and the counts, from diagnostics (with offsets) and the ranges
    being processed (None when unknown, as when nothing has been reported)"""
    n = len(commands)
    base = ["finished"] * n
    for d in diags:
        i = anchor_command(commands, d["start"])
        if i is None:
            continue
        sev = d.get("severity", 1)
        if sev == 1:
            base[i] = "failed"
        elif sev == 2 and base[i] != "failed":
            base[i] = "warned"
    states = list(base)
    first = None
    if processing is None:
        processing = [(0, 1 << 60)]
    live = [(s, e) for s, e in processing if e > s]
    if live:
        for i, (s, e, _name) in enumerate(commands):
            if any(s < pe and ps < e for ps, pe in live):
                if first is None:
                    first = i
                    states[i] = "running"
                else:
                    states[i] = "unprocessed"
    count = {k: states.count(k) for k in ("unprocessed", "running", "warned", "failed", "finished")}
    done = not live
    reached = n - count["unprocessed"] - count["running"]
    status = dict(count, total=n, canceled=0, consolidated=done,
                  percentage=100 if n == 0 else int(100 * reached / n))
    return {"status": status, "commands": [[s, e, name, st] for (s, e, name), st in zip(commands, states)],
            "running": first, "done": done}


# Lean's semantic token types as Isabelle's colour classes the app knows (the others stay plain)
TOKEN_CLASSES = {
    "keyword": "keyword1", "modifier": "keyword2", "decorator": "keyword2", "macro": "keyword3",
    "variable": "free", "parameter": "bound", "typeParameter": "tfree", "property": "class_parameter",
    "enumMember": "dynamic", "leanSorryLike": "improper", "operator": "operator", "string": "inner_quoted",
    "comment": "comment1",
}


def decode_tokens(data: List[int], legend: List[str], index: TextIndex) -> List[List[Any]]:
    """``[[start, end, class]]`` from LSP's relative semantic token data"""
    out: List[List[Any]] = []
    line = char = 0
    for i in range(0, len(data) - 4, 5):
        dl, dc, length, ttype = data[i], data[i + 1], data[i + 2], data[i + 3]
        line += dl
        char = dc if dl else char + dc
        cls = TOKEN_CLASSES.get(legend[ttype]) if ttype < len(legend) else None
        if cls:
            out.append([index.to_offset(line, char), index.to_offset(line, char + length), cls])
    return out


def lexical_colours(mask: bytearray) -> List[List[Any]]:
    """Comments and strings, which the server's tokens leave out"""
    out: List[List[Any]] = []
    classes = {COMMENT: "comment1", DOC: "comment1", MODDOC: "comment1", STRING: "inner_quoted"}
    i, n = 0, len(mask)
    while i < n:
        k = mask[i]
        if k == CODE:
            i += 1
            continue
        j = i
        while j < n and mask[j] == k:
            j += 1
        out.append([i, j, classes[k]])
        i = j
    return out


# ---------------------------------------------------------------------------
# The proof state in parts


_FENCE = re.compile(r"\A\s*```[A-Za-z0-9_-]*\n(.*?)\n?```\s*\Z", re.S)


def tooltip_items(markdown: str) -> List[str]:
    """The pieces of a Lean hover, which is Markdown: a signature in a code fence, then, after a
    ``***`` line, the docstring. The fence is taken off (the app shows the item as code) and each
    piece is an item of its own."""
    out = []
    for part in re.split(r"\n\s*\*\*\*\s*\n", markdown.strip()):
        m = _FENCE.match(part)
        text = (m.group(1) if m else part).strip("\n")
        if text.strip():
            out.append(text)
    return out


def tagged_text(tt: Any) -> str:
    """Plain text of a ``TaggedText`` (what the infoview renders)"""
    if tt is None:
        return ""
    if isinstance(tt, str):
        return tt
    if "text" in tt:
        return tt["text"]
    if "append" in tt:
        return "".join(tagged_text(x) for x in tt["append"])
    if "tag" in tt:
        return tagged_text(tt["tag"][1])
    return ""


def goal_from_interactive(g: Dict[str, Any]) -> Dict[str, Any]:
    hyps = []
    for h in g.get("hyps", []):
        hyps.append({"names": list(h.get("names", [])), "type": tagged_text(h.get("type")),
                     "value": tagged_text(h["val"]) if h.get("val") is not None else None})
    return {"tag": g.get("userName") or None, "hyps": hyps, "target": tagged_text(g.get("type")),
            "prefix": g.get("goalPrefix") or "⊢ "}


def parse_goal_text(s: str) -> Dict[str, Any]:
    """A goal as the infoview prints it (``case tag``, hypotheses, ``⊢ target``) back into parts"""
    lines = s.rstrip("\n").split("\n")
    tag = None
    if lines and lines[0].startswith("case "):
        tag = lines.pop(0)[5:].strip() or None
    hyps: List[Dict[str, Any]] = []
    target: List[str] = []
    in_target = False
    for ln in lines:
        if in_target:
            target.append(ln)
        elif ln.startswith("⊢"):
            in_target = True
            target.append(ln[1:].lstrip())
        elif ln[:1] in (" ", "\t") and hyps:
            hyps[-1]["raw"] += "\n" + ln
        else:
            hyps.append({"raw": ln})
    out_hyps = []
    for h in hyps:
        names, sep, rest = h["raw"].partition(" : ")
        if sep:
            out_hyps.append({"names": names.split(), "type": rest, "value": None})
        else:
            out_hyps.append({"names": [], "type": h["raw"], "value": None})
    return {"tag": tag, "hyps": out_hyps, "target": "\n".join(target), "prefix": "⊢ "}


def hyp_text(h: Dict[str, Any]) -> str:
    head = " ".join(h["names"])
    body = h["type"] if not head else head + " : " + h["type"]
    if h.get("value") is not None:
        body += " := " + h["value"]
    return body


def goal_text(g: Dict[str, Any]) -> str:
    """One goal as the VS Code infoview shows it"""
    lines = []
    if g.get("tag"):
        lines.append("case " + g["tag"])
    lines.extend(hyp_text(h) for h in g["hyps"])
    lines.append(g.get("prefix", "⊢ ") + g["target"])
    return "\n".join(lines)


def goals_text(goals: List[Dict[str, Any]]) -> str:
    return "\n\n".join(goal_text(g) for g in goals) if goals else "No goals"


def goal_parts(goals: List[Dict[str, Any]], nogoal: bool = False) -> List[Dict[str, Any]]:
    """The ``deductum_goals`` messages for the goals (design §9.1; protocol "Goal parts")"""
    def part(kind: str, n: int, body: str) -> Dict[str, Any]:
        head = f"#deductum:{kind} {n}\n"
        return message("writeln", head + body)

    out = [part("count", len(goals), "")]
    if nogoal and not goals:
        out = [part("nogoal", 0, "No goals")]
    for i, g in enumerate(goals, 1):
        if g.get("tag"):
            out.append(part("case", i, g["tag"]))
        for h in g["hyps"]:
            out.append(part("param", i, hyp_text(h)))
        out.append(part("concl", i, g["target"]))
    out.append(part("text", 0, goals_text(goals)))
    return out


# ---------------------------------------------------------------------------
# Loogle: searching Lean and Mathlib by name or type pattern

LOOGLE_URL = "https://loogle.lean-lang.org/json"
LOOGLE_TIMEOUT = 15.0
LOOGLE_LIMIT = 40


def loogle_fetch(query: str, source: str, timeout: float = LOOGLE_TIMEOUT) -> Dict[str, Any]:
    """Loogle's answer for ``query`` (blocking): ``public`` asks loogle.lean-lang.org, ``local`` runs
    a ``loogle`` binary on the PATH. Raises ``HostError`` with a message fit for the human."""
    query = query.strip()
    if not query:
        raise HostError("Type a name, a pattern of the type (such as ?a + ?b = ?b + ?a) or a word of the name")
    if source == "off":
        raise HostError("Search is off in the settings")
    if source == "local":
        binary = shutil.which("loogle")
        if not binary:
            raise HostError("There is no loogle program on the PATH of this machine "
                            "(github.com/nomeata/loogle); choose the public Loogle in the settings, or install it")
        try:
            run = subprocess.run([binary, "--json", query], capture_output=True, text=True, timeout=timeout)
        except subprocess.TimeoutExpired:
            raise HostError(f"The local loogle did not answer within {timeout:g} seconds")
        if run.returncode != 0 and not run.stdout.strip():
            raise HostError("loogle failed: " + (run.stderr.strip().splitlines() or ["no output"])[-1])
        raw = run.stdout
    elif source == "public":
        url = LOOGLE_URL + "?" + urllib.parse.urlencode({"q": query})
        try:
            req = urllib.request.Request(url, headers={"User-Agent": "deductum", "Accept": "application/json"})
            with urllib.request.urlopen(req, timeout=timeout) as res:
                raw = res.read().decode("utf-8", "replace")
        except urllib.error.HTTPError as exn:
            raise HostError(f"loogle.lean-lang.org answered {exn.code}")
        except (urllib.error.URLError, TimeoutError, OSError) as exn:
            raise HostError(f"loogle.lean-lang.org could not be reached ({getattr(exn, 'reason', exn)})")
    else:
        raise HostError(f"Unknown search source: {source}")
    try:
        result = json.loads(raw)
    except ValueError:
        raise HostError("loogle answered something that is not JSON")
    if not isinstance(result, dict):
        raise HostError("loogle answered something unexpected")
    return result


def loogle_messages(result: Dict[str, Any], source: str, limit: int = LOOGLE_LIMIT) -> List[Dict[str, Any]]:
    """The messages the app lists for Loogle's answer: a heading naming the source, then one per hit
    (``name``, ``type``, ``module``; Insert puts the name at the caret)"""
    where = {"public": "loogle.lean-lang.org", "local": "local loogle"}.get(source, source)
    if result.get("error"):
        out = [message("error", f"{result['error']}")]
        for s in (result.get("suggestions") or [])[:5]:
            out.append(message("writeln", f"Did you mean: {s}"))
        return out
    hits = result.get("hits") or []
    count = result.get("count", len(hits))
    head = f"{count} declaration{'s' if count != 1 else ''} from {where}" + (f" (showing {limit})" if count > limit else "")
    out = [message("writeln", head)]
    for h in hits[:limit]:
        name, ty, mod = h.get("name", ""), h.get("type", ""), h.get("module", "")
        out.append(message("writeln", f"{name} : {ty}\n{mod}".rstrip(), name=name, type=ty, module=mod,
                           doc=h.get("doc") or "", sendbacks=[name]))
    return out


# ---------------------------------------------------------------------------
# Discovery without a session


_VERSION = re.compile(r"v?(\d+)\.(\d+)\.(\d+)(?:-?(rc\d+))?")


def version_tuple(s: Optional[str]) -> Optional[Tuple[int, int, int]]:
    m = _VERSION.search(s or "")
    return (int(m.group(1)), int(m.group(2)), int(m.group(3))) if m else None


def find_elan() -> Optional[str]:
    p = os.path.join(ELAN_BIN, "elan")
    return p if os.access(p, os.X_OK) else shutil.which("elan")


def tool_env() -> Dict[str, str]:
    """The environment for elan's tools: PATH with ``~/.elan/bin`` (a Flatpak or service may lack it)"""
    env = dict(os.environ)
    env["PATH"] = ELAN_BIN + os.pathsep + env.get("PATH", "")
    return env


def toolchain_dir(tid: str) -> str:
    return os.path.join(HOME, ".elan", "toolchains", tid.replace("/", "--").replace(":", "---"))


def read_text(path: str) -> Optional[str]:
    with contextlib.suppress(OSError, UnicodeDecodeError):
        return Path(path).read_text(encoding="utf-8")
    return None


def lake_project(root: str) -> Dict[str, Any]:
    """What the Lake project at ``root`` is made of"""
    toolchain = (read_text(os.path.join(root, "lean-toolchain")) or "").strip() or None
    packages: List[str] = []
    manifest = read_text(os.path.join(root, "lake-manifest.json"))
    if manifest:
        with contextlib.suppress(ValueError):
            packages = [p.get("name", "") for p in json.loads(manifest).get("packages", []) if p.get("name")]
    lakefile = (read_text(os.path.join(root, "lakefile.toml")) or "") + (read_text(os.path.join(root, "lakefile.lean")) or "")
    mathlib = any(p.lower() == "mathlib" for p in packages) or bool(re.search(r"mathlib", lakefile, re.I))
    return {"lake_root": root, "toolchain": toolchain, "packages": packages, "mathlib": mathlib}


# ---------------------------------------------------------------------------
# The driver


class LDoc:
    """One document as the driver has it: the host's text and revision, and what the server said"""

    def __init__(self, path: str, text: str, version: int):
        self.path = path
        self.uri = Path(path).as_uri()
        self.text = text
        self.version = version
        self.server_open = False
        self.history: Dict[int, str] = {version: text}
        self.splits: Dict[int, Splitter] = {}
        self.diags: Optional[Tuple[int, List[Dict[str, Any]]]] = None  # version, with offsets
        self.progress: Optional[Tuple[int, List[Tuple[int, int]]]] = None
        self.colours: Optional[Tuple[int, List[List[Any]]]] = None
        self.stale = False
        self.rpc: Optional[str] = None
        self.rpc_task: Optional[asyncio.Task] = None
        self.node_handle: Optional[asyncio.TimerHandle] = None
        self.colour_task: Optional[asyncio.Task] = None
        self.report: Optional[Dict[str, Any]] = None
        self.report_seq = 0

    def remember(self) -> None:
        self.history[self.version] = self.text
        self.splits.pop(self.version, None)
        for v in sorted(self.history)[:-16]:
            self.history.pop(v, None)
            self.splits.pop(v, None)

    def splitter(self, version: Optional[int] = None) -> Splitter:
        v = self.version if version is None else version
        sp = self.splits.get(v)
        if sp is None:
            sp = self.splits[v] = Splitter(self.history[v])
            for old in sorted(self.splits)[:-4]:
                self.splits.pop(old, None)
        return sp


class Scratch:
    """A document opened only in the server, to check a candidate"""

    def __init__(self, uri: str, text: str):
        self.uri = uri
        self.text = text
        self.diags: List[Dict[str, Any]] = []
        self.done = asyncio.Event()
        self.index = TextIndex(text)


def pick_range(rng: Dict[str, Any], index: TextIndex) -> Tuple[int, int]:
    return index.range_to_offsets(rng)


class LeanDriver(Driver):
    backend = "lean"
    label = "Lean 4"
    documents = (".lean",)
    project_files = ("lakefile.lean", "lakefile.toml", "lean-toolchain", "lake-manifest.json")
    placeholder = "sorry"
    placeholder_commands = ("sorry",)
    goal_commands = GOAL_KEYWORDS
    theory_end_commands = ("end",)
    tool_names = TOOL_TACTICS
    option_allowlist = ()
    statement_regex = re.compile(r"\s*(lemma|theorem|example|corollary|proposition)\b\s*([^\s:({\[]+)?")
    proof_start_regex = re.compile(r"\n\s*by\b|:=")
    facts_line = "simp [{facts}]"
    facts_separator = ", "
    fork_links = (".lake/packages",)
    open_policy = "viewed"

    NODE_DEBOUNCE = 0.1
    KEEPALIVE = 5.0

    def __init__(self, session: Any):
        super().__init__(session)
        self.client: Optional[LspClient] = None
        self.docs: Dict[str, LDoc] = {}
        self.by_uri: Dict[str, LDoc] = {}
        self.scratch: Dict[str, Scratch] = {}
        self.queries: Dict[str, asyncio.Task] = {}
        self.paused = False
        self.stopping = False
        self.lake_root = ""
        self.project: Dict[str, Any] = {}
        self.version = ""
        self.legend: List[str] = []
        self.check_slots: Optional[asyncio.Semaphore] = None
        self.stderr_tail: List[str] = []
        self.build_task: Optional[asyncio.Task] = None

    # ----- discovery

    @classmethod
    def toolchains(cls) -> List[Dict[str, Any]]:
        elan = find_elan()
        if not elan:
            return []
        import subprocess
        try:
            out = subprocess.run([elan, "toolchain", "list"], capture_output=True, text=True, timeout=30,
                                 env=tool_env()).stdout
        except (OSError, subprocess.SubprocessError):
            return []
        found = []
        for line in out.splitlines():
            parts = line.split()
            if not parts or not re.fullmatch(r"[\w.-]+/[\w.-]+:[\w.-]+|stable|nightly|beta", parts[0]):
                continue
            tid = parts[0]
            vt = version_tuple(tid)
            version = ".".join(map(str, vt)) if vt else None
            found.append({"id": tid, "label": "Lean v" + version if version else "Lean " + tid.split(":")[-1],
                          "version": version, "path": toolchain_dir(tid), "supported": True, "problem": None})
        return found

    @classmethod
    def detect(cls, folder: str) -> Optional[Dict[str, Any]]:
        d = os.path.realpath(folder)
        home = os.path.realpath(HOME)
        for _ in range(16):
            for name in ("lakefile.toml", "lakefile.lean"):
                if os.path.isfile(os.path.join(d, name)):
                    return cls._detected(d, name)
            if d == home:
                break
            parent = os.path.dirname(d)
            if parent == d:
                break
            d = parent
        return None

    @classmethod
    def _detected(cls, root: str, lakefile: str) -> Dict[str, Any]:
        project = lake_project(root)
        evidence = [f"{lakefile} in {root}"]
        if project["toolchain"]:
            evidence.append("lean-toolchain: " + project["toolchain"])
        if project["packages"]:
            evidence.append("packages: " + ", ".join(project["packages"]))
        notes = []
        want = version_tuple(project["toolchain"])
        have = [version_tuple(t["id"]) for t in cls.toolchains()]
        if project["toolchain"] and want and want not in have:
            notes.append(f"elan will download {project['toolchain']} when the project starts")
        if project["mathlib"]:
            built = os.path.isdir(os.path.join(root, ".lake", "packages", "mathlib", ".lake", "build"))
            if not built:
                notes.append("Mathlib is not built here; Get Mathlib cache fetches its compiled files")
        return {"backend": "lean", "evidence": evidence, "project": project,
                "toolchain": project["toolchain"], "note": "; ".join(notes) or None}

    @classmethod
    def resolve_session(cls, params: Dict[str, Any], folder: str) -> Dict[str, Any]:
        """The project's own toolchain when it is installed, else the newest installed one"""
        project = params.get("project")
        if project is None:
            project = (cls.detect(folder) or {}).get("project") or {}
        toolchain = params.get("toolchain")
        if not toolchain:
            installed = [t for t in cls.toolchains() if t.get("supported", True)]
            want = version_tuple(project.get("toolchain"))
            match = [t for t in installed if want and version_tuple(t["id"]) == want]
            if match:
                toolchain = match[0]["id"]
            elif project.get("toolchain"):
                toolchain = project["toolchain"]  # (elan downloads it)
            elif installed:
                toolchain = installed[-1]["id"]
            else:
                raise HostError("No Lean toolchain is installed (install elan)", "unsupported_toolchain")
        return {"toolchain": toolchain, "project": dict(project), "meta": {}}

    @classmethod
    def project_key(cls, project: Dict[str, Any]) -> str:
        return project.get("lake_root", "")

    @classmethod
    def relocate_project(cls, project: Dict[str, Any], moved: Callable[[str], str]) -> Dict[str, Any]:
        out = dict(project)
        if out.get("lake_root"):
            out["lake_root"] = moved(out["lake_root"])
        return out

    @classmethod
    def toolchain_label(cls, toolchain: str, meta: Dict[str, Any]) -> str:
        vt = version_tuple(toolchain)
        return "Lean v" + ".".join(map(str, vt)) if vt else "Lean " + toolchain.split(":")[-1]

    @classmethod
    def header_of(cls, text: str) -> str:
        """The leading ``import``/``open`` lines (with comments between them)"""
        end = 0
        for m in re.finditer(r"^(?:import|open|set_option|universe|prelude)\b.*$|^\s*$|^--.*$", text, re.M):
            if m.start() != end and text[end:m.start()].strip():
                break
            end = m.end()
        return text[:end].rstrip() or text[:400]

    def building_message(self) -> str:
        return "Starting the Lean server (lake serve)"

    # ----- capabilities

    def capabilities(self) -> Dict[str, Any]:
        vt = version_tuple(self.version) or version_tuple(self.project.get("toolchain")) or (0, 0, 0)
        mathlib = bool(self.project.get("mathlib"))
        packages = [p.lower() for p in self.project.get("packages", [])]
        tools = ["exact?", "apply?", "rw?", "simp?", "decide", "omega"]
        if vt >= (4, 22, 0):
            tools.append("grind")
        if mathlib or "aesop" in packages:
            tools.append("aesop?")
        if mathlib:
            tools.append("hint")
        actions = [{"name": "build", "label": "Build (lake build)"},
                   {"name": "rebuild_imports", "label": "Rebuild imports"}]
        if mathlib:
            actions.append({"name": "cache_get", "label": "Get Mathlib cache"})
        return {
            "checking": "continuous", "pause": "close_documents", "interrupt": True,
            "goals": {"structured": True, "term_goal": True, "facts": False, "case_tags": True},
            "hover": True, "definition": True, "spans": "lexical",
            "tools": [{"name": t, "typed": t, "kind": "prove", "stop": False} for t in tools],
            "counterexamples": ["plausible"] if mathlib or "plausible" in packages else [],
            "search": {"label": "Search", "tab": "Search", "syntax": "loogle", "operation": "loogle", "args": [],
                       "sources": ["exact?", "apply?", "loogle"],
                       "placeholder": "Nat.add_comm   or   ?a + ?b = ?b + ?a   or   \"comm\"",
                       "note": "Loogle: by name, by a pattern of the type (?a, ?b stand for anything), or by a word of the name. Its source is a setting."},
            "command": ["#check", "#eval", "#print", "#reduce"],
            "check_candidates": "scratch", "project_actions": actions, "semantic_colours": True,
            "documents": list(self.documents), "project_files": list(self.project_files),
            "placeholder": self.placeholder, "facts_line": "simp [{facts}]", "goal_style": "lean",
        }

    # ----- hooks for the core

    @classmethod
    def fact_check_text(cls, facts: List[str]) -> Optional[str]:
        """A command text that fails if one of the facts does not exist (for ``deductum_command``)"""
        names = [f for f in facts if f.strip()]
        return "\n".join("#check @" + f.strip() for f in names) if names else None

    async def project_action(self, name: str, args: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
        """Run a project action in the Lake root, streaming its output as ``log`` events"""
        args = args if isinstance(args, dict) else {"file": args[0]} if args else {}
        if name == "rebuild_imports":
            files = [args["file"]] if args.get("file") in self.docs else list(self.docs)
            for f in files:
                self._reopen(self.docs[f])
            return {"ok": True, "output_tail": f"Reopened {len(files)} file(s)"}
        commands = {"build": ["lake", "build"], "cache_get": ["lake", "exe", "cache", "get"]}
        if name not in commands:
            return {"ok": False, "output_tail": "Unknown project action: " + name}
        if self.build_task and not self.build_task.done():
            return {"ok": False, "output_tail": "Another project action is running"}
        self.build_task = asyncio.current_task()
        env = tool_env()
        self.emit({"event": "log", "text": "$ " + " ".join(commands[name])})
        try:
            proc = await asyncio.create_subprocess_exec(
                *commands[name], cwd=self.lake_root, env=env, stdin=asyncio.subprocess.DEVNULL,
                stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.STDOUT, start_new_session=True)
        except OSError as exn:
            return {"ok": False, "output_tail": f"Cannot run {commands[name][0]}: {exn}"}
        tail: List[str] = []
        assert proc.stdout
        while True:
            raw = await proc.stdout.readline()
            if not raw:
                break
            line = raw.decode("utf-8", "replace").rstrip()
            tail = (tail + [line])[-40:]
            self.emit({"event": "log", "text": line})
        code = await proc.wait()
        if code == 0:
            for doc in self.docs.values():
                if doc.stale or doc.diags and any(d["message"].startswith("unknown module") for d in doc.diags[1]):
                    self._reopen(doc)
        return {"ok": code == 0, "output_tail": "\n".join(tail)}

    # ----- starting and stopping

    async def start(self) -> None:
        s = self.session
        meta = getattr(s, "meta", {}) or {}
        folder = meta.get("folder") or os.getcwd()
        project = meta.get("project")
        if not project or not project.get("lake_root"):
            found = self.detect(folder)
            if not found:
                return self._fail(f"{folder} is not inside a Lake project (no lakefile.toml or lakefile.lean)")
            project = found["project"]
        self.project = project
        self.lake_root = project["lake_root"]
        env = tool_env()
        lake = shutil.which("lake", path=env["PATH"])
        if not lake:
            return self._fail("Lean's build tool `lake` was not found. Install elan "
                              "(https://github.com/leanprover/elan) and open the project again.")
        want = meta.get("toolchain") or project.get("toolchain")
        if meta.get("toolchain") and meta["toolchain"] != project.get("toolchain"):
            env["ELAN_TOOLCHAIN"] = meta["toolchain"]
        toolchains = await asyncio.to_thread(self.toolchains) if want else []
        if want and version_tuple(want) not in [version_tuple(t["id"]) for t in toolchains]:
            self.emit({"event": "phase", "phase": "building", "message": f"Installing the Lean toolchain {want}"})
        self.check_slots = asyncio.Semaphore(resource_limit(s, "scratch_checks", 2))
        server_args = ["--", "-j", str(threads)] if (threads := resource_limit(s, "prover_threads")) else []
        try:
            self.proc = await asyncio.create_subprocess_exec(
                lake, "serve", *server_args, cwd=self.lake_root, env=env, stdin=asyncio.subprocess.PIPE,
                stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE, start_new_session=True,
                limit=64 * 1024 * 1024)
        except OSError as exn:
            return self._fail(f"Could not start lake: {exn}")
        self.client = LspClient(self.proc, on_notification=self._on_notification, on_request=self._on_request,
                                on_exit=self._on_exit, on_stderr=self._on_stderr)
        self.version = await self._lean_version(lake, env)
        try:
            result = await self.client.request("initialize", self._initialize_params(), timeout=1800)
        except (LspClosed, LspError, asyncio.TimeoutError) as exn:
            tail = "\n".join(self.stderr_tail[-8:])
            return self._fail(f"The Lean server did not start ({exn or type(exn).__name__})" + (": " + tail if tail else ""))
        self.legend = (((result or {}).get("capabilities") or {}).get("semanticTokensProvider") or {}) \
            .get("legend", {}).get("tokenTypes", [])
        self.client.notify("initialized", {})
        self.ready = True
        self.emit({"event": "ready", "identifier": "Lean " + (self.version or "4"), "home": self.lake_root})

    def _fail(self, text: str) -> None:
        self.emit({"event": "phase", "phase": "failed", "message": text})

    async def _lean_version(self, lake: str, env: Dict[str, str]) -> str:
        try:
            proc = await asyncio.create_subprocess_exec(
                lake, "env", "lean", "--version", cwd=self.lake_root, env=env, stdin=asyncio.subprocess.DEVNULL,
                stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.DEVNULL)
            out, _ = await asyncio.wait_for(proc.communicate(), 900)
        except (OSError, asyncio.TimeoutError):
            return ""
        m = re.search(r"version (\S+?),", out.decode("utf-8", "replace"))
        return m.group(1) if m else ""

    def _initialize_params(self) -> Dict[str, Any]:
        root = Path(self.lake_root).as_uri()
        return {
            "processId": os.getpid(), "clientInfo": {"name": "Deductum"}, "rootUri": root,
            "workspaceFolders": [{"uri": root, "name": os.path.basename(self.lake_root)}],
            "capabilities": {
                "textDocument": {
                    "publishDiagnostics": {"versionSupport": True, "relatedInformation": False},
                    "hover": {"contentFormat": ["markdown", "plaintext"]},
                    "definition": {"linkSupport": True},
                    "semanticTokens": {"requests": {"full": True}, "formats": ["relative"],
                                       "tokenTypes": [], "tokenModifiers": []},
                    "codeAction": {"codeActionLiteralSupport": {"codeActionKind": {"valueSet": ["quickfix"]}}},
                    "synchronization": {"didSave": False},
                },
                "workspace": {"didChangeWatchedFiles": {"dynamicRegistration": True},
                              "semanticTokens": {"refreshSupport": True}, "configuration": True},
                "window": {"workDoneProgress": True},
            },
            "initializationOptions": {"hasWidgets": True},
        }

    async def stop(self) -> None:
        self.stopping = True
        self.ready = False
        for task in list(self.queries.values()):
            task.cancel()
        for doc in self.docs.values():
            self._cancel_doc_tasks(doc)
        if self.client:
            await self.client.shutdown()
        elif self.proc:
            from . import stop_process
            await stop_process(self.proc)

    def _cancel_doc_tasks(self, doc: LDoc) -> None:
        for t in (doc.rpc_task, doc.colour_task):
            if t:
                t.cancel()
        doc.rpc_task = doc.colour_task = None
        doc.rpc = None
        if doc.node_handle:
            doc.node_handle.cancel()
            doc.node_handle = None

    def _on_exit(self, code: Optional[int]) -> None:
        self.ready = False
        if not self.stopping:
            tail = "\n".join(self.stderr_tail[-8:])
            with contextlib.suppress(Exception):
                self.session.on_backend_exit(self, code)
            self._fail(f"The Lean server stopped (exit code {code})" + (": " + tail if tail else ""))

    def _on_stderr(self, line: str) -> None:
        self.stderr_tail = (self.stderr_tail + [line])[-40:]
        self.emit({"event": "log", "text": line})

    # ----- the protocol: operations from the core

    def send(self, obj: Dict[str, Any]) -> None:
        op = obj.get("op")
        handler = getattr(self, "_send_" + str(op), None)
        if handler:
            handler(obj)

    async def request(self, obj: Dict[str, Any], timeout: float = 120.0) -> Dict[str, Any]:
        op = obj.get("op")
        handler = getattr(self, "_req_" + str(op), None)
        if not handler:
            return {"error": f"Unknown operation: {op}"}
        try:
            return await asyncio.wait_for(handler(obj), timeout)
        except asyncio.TimeoutError:
            return {"error": f"{op} timed out"}
        except LspClosed:
            return {"error": "The Lean server is not running"}
        except LspError as exn:
            return {"error": f"Lean server error: {exn.message}"}

    def _notify(self, method: str, params: Any) -> None:
        if self.client:
            self.client.notify(method, params)

    # ----- documents

    def _send_open(self, obj: Dict[str, Any]) -> None:
        path = obj["file"]
        doc = self.docs.get(path)
        if doc is not None and doc.server_open:
            self._did_close(doc)
        doc = self.docs[path] = LDoc(path, obj["text"], int(obj.get("rev", 1)))
        self.by_uri[doc.uri] = doc
        doc.progress = (doc.version, [(0, len(doc.text))])
        if not self.paused:
            self._did_open(doc)
        self._schedule_node(doc)

    def _did_open(self, doc: LDoc) -> None:
        doc.server_open = True
        self._notify("textDocument/didOpen", {"textDocument": {
            "uri": doc.uri, "languageId": "lean4", "version": doc.version, "text": doc.text}})

    def _did_close(self, doc: LDoc) -> None:
        if doc.server_open:
            doc.server_open = False
            self._notify("textDocument/didClose", {"textDocument": {"uri": doc.uri}})
        self._cancel_doc_tasks(doc)

    def _reopen(self, doc: LDoc) -> None:
        """Close and open the document again: the server starts over (and runs ``lake setup-file``)"""
        if self.paused or not doc.server_open:
            return
        self._did_close(doc)
        doc.stale = False
        doc.diags = None
        doc.progress = (doc.version, [(0, len(doc.text))])
        self._did_open(doc)
        self._schedule_node(doc)

    def _send_edit(self, obj: Dict[str, Any]) -> None:
        doc = self.docs.get(obj["file"])
        if doc is None:
            return
        changes = []
        first = len(doc.text)
        for ed in obj.get("edits", []):
            s, e, new = int(ed["start"]), int(ed["end"]), ed["text"]
            index = TextIndex(doc.text)
            changes.append({"range": index.range_to_lsp(s, e), "text": new})
            doc.text = doc.text[:s] + new + doc.text[e:]
            first = min(first, s)
        doc.version = int(obj.get("rev", doc.version + 1))
        doc.remember()
        if doc.server_open and changes:
            self._notify("textDocument/didChange", {"textDocument": {"uri": doc.uri, "version": doc.version},
                                                    "contentChanges": changes})
        # What Lean will redo, until it says: from the command that was touched to the end
        sp = doc.splitter()
        i = anchor_command(sp.commands, min(first, len(doc.text)))
        begin = sp.commands[i][0] if i is not None else 0
        if not self.paused:
            doc.progress = (doc.version, [(min(begin, first), len(doc.text))])
        self._schedule_node(doc)

    def _send_close(self, obj: Dict[str, Any]) -> None:
        doc = self.docs.pop(obj["file"], None)
        if doc:
            self._did_close(doc)
            self.by_uri.pop(doc.uri, None)

    def _send_disk_changed(self, obj: Dict[str, Any]) -> None:
        """Files changed on disk that the session has not open: tell the server, which marks the
        open files that import them (a sticky "imports are out of date" message)"""
        changes = [{"uri": Path(p).as_uri(), "type": 2} for p in obj.get("files", [])
                   if p.endswith(".lean") and p not in self.docs]
        if changes:
            self._notify("workspace/didChangeWatchedFiles", {"changes": changes})

    def _send_checking(self, obj: Dict[str, Any]) -> None:
        on = bool(obj.get("on"))
        if on == (not self.paused):
            return
        self.paused = not on
        for doc in self.docs.values():
            if self.paused:
                self._did_close(doc)
            else:
                doc.diags = None
                doc.progress = (doc.version, [(0, len(doc.text))])
                self._did_open(doc)
                self._schedule_node(doc)

    async def _req_checking(self, obj: Dict[str, Any]) -> Dict[str, Any]:
        self._send_checking(obj)
        return {}

    async def _req_rebuild_imports(self, obj: Dict[str, Any]) -> Dict[str, Any]:
        doc = self.docs.get(obj.get("file", ""))
        if doc is None:
            return {"error": "That file is not open"}
        self._reopen(doc)
        return {}

    def _busy(self, doc: LDoc) -> bool:
        return bool(doc.server_open and doc.progress and any(e > s for s, e in doc.progress[1]))

    async def _req_cancel(self, obj: Dict[str, Any]) -> Dict[str, Any]:
        """No request stops Lean's elaboration, so the files that are being checked are closed and opened"""
        busy = [d for d in self.docs.values() if self._busy(d)]
        for task in list(self.queries.values()):
            task.cancel()
        for doc in busy:
            self._reopen(doc)
        return {"canceled": len(busy)}

    async def _req_cancel_tools(self, obj: Dict[str, Any]) -> Dict[str, Any]:
        for doc in self.docs.values():
            rep = doc.report
            if rep and rep.get("running") is not None and self._busy(doc):
                name = rep["commands"][rep["running"]][2]
                if name in TOOL_TACTICS or name.endswith("?"):
                    self._reopen(doc)
        return {}

    def _send_cancel_query(self, obj: Dict[str, Any]) -> None:
        task = self.queries.get(obj.get("query", ""))
        if task:
            task.cancel()

    async def _req_ping(self, obj: Dict[str, Any]) -> Dict[str, Any]:
        return {}

    async def _req_options(self, obj: Dict[str, Any]) -> Dict[str, Any]:
        return {}

    async def _req_symbols(self, obj: Dict[str, Any]) -> Dict[str, Any]:
        return {"symbols": []}

    async def _req_syntax(self, obj: Dict[str, Any]) -> Dict[str, Any]:
        return {"syntax": {"kinds": dict(LEAN_KINDS)}}

    # ----- what the server says

    def _on_notification(self, method: str, params: Any) -> None:
        params = params or {}
        if method == "textDocument/publishDiagnostics":
            self._on_diagnostics(params)
        elif method == "$/lean/fileProgress":
            self._on_progress(params)
        elif method == "$/lean/staleDependency":
            doc = self.by_uri.get((params.get("uri") or ""))
            if doc:
                doc.stale = True
                self._schedule_node(doc)
        elif method in ("window/logMessage", "window/showMessage"):
            text = params.get("message", "")
            if text:
                self.emit({"event": "log", "text": text})
        elif method == "$/progress":
            value = params.get("value") or {}
            if value.get("kind") in ("begin", "end"):
                text = " ".join(x for x in (value.get("title"), value.get("message")) if x)
                if text:
                    self.emit({"event": "log", "text": text})

    def _on_request(self, method: str, params: Any) -> Any:
        if method == "workspace/semanticTokens/refresh":
            for doc in self.docs.values():
                if doc.server_open:
                    self._schedule_colours(doc)
            return None
        if method == "window/showMessageRequest":
            return None
        return UNHANDLED

    def _doc_text(self, doc: LDoc, version: Optional[int]) -> Optional[str]:
        return doc.history.get(version) if version is not None else doc.text

    def _on_diagnostics(self, params: Dict[str, Any]) -> None:
        uri = params.get("uri", "")
        sc = self.scratch.get(uri)
        if sc is not None:
            sc.diags = params.get("diagnostics", [])
            return
        doc = self.by_uri.get(uri)
        if doc is None or not doc.server_open:
            return
        version = params.get("version", doc.version)
        text = doc.history.get(version)
        if text is None or (doc.diags and version < doc.diags[0]):
            return
        index = doc.splitter(version).index
        out = []
        for d in params.get("diagnostics", []):
            s, e = index.range_to_offsets(d["range"])
            if d.get("message", "").startswith(STALE_MESSAGE):
                doc.stale = True
                continue
            out.append({"severity": d.get("severity", 1), "start": s, "end": e, "message": d.get("message", "")})
        doc.diags = (version, out)
        self._schedule_node(doc)

    def _on_progress(self, params: Dict[str, Any]) -> None:
        td = params.get("textDocument") or {}
        uri = td.get("uri", "")
        sc = self.scratch.get(uri)
        if sc is not None:
            if not params.get("processing"):
                asyncio.get_running_loop().call_later(0.25, sc.done.set)
            return
        doc = self.by_uri.get(uri)
        if doc is None or not doc.server_open:
            return
        version = td.get("version", doc.version)
        if doc.history.get(version) is None or (doc.progress and version < doc.progress[0]):
            return
        index = doc.splitter(version).index
        ranges = [index.range_to_offsets(p["range"]) for p in params.get("processing", [])]
        doc.progress = (version, ranges)
        if not any(e > s for s, e in ranges):
            self._schedule_colours(doc)
        self._schedule_node(doc)

    # ----- node events

    def _schedule_node(self, doc: LDoc) -> None:
        if doc.node_handle is None:
            doc.node_handle = asyncio.get_running_loop().call_later(self.NODE_DEBOUNCE, self._emit_node, doc)

    def _emit_node(self, doc: LDoc) -> None:
        doc.node_handle = None
        try:
            node = self.node_for(doc)
        except Exception as exn:  # a report that cannot be made must not stop the next
            print("lean node error", repr(exn), file=sys.stderr, flush=True)
            return
        if node:
            self.emit(node)

    def node_for(self, doc: LDoc) -> Optional[Dict[str, Any]]:
        """The report on a document, for the newest version the server has said anything about"""
        versions = [v for v in (doc.diags and doc.diags[0], doc.progress and doc.progress[0]) if v is not None]
        if not versions:
            return None
        seq = max(versions)
        text = doc.history.get(seq)
        if text is None:
            return None
        sp = doc.splitter(seq)

        def carried(version: int, spans: List[Tuple[int, int]]) -> List[Optional[Tuple[int, int]]]:
            if version == seq:
                return list(spans)
            if version not in doc.history:
                return [None for _ in spans]
            tm = TextMap(doc.history[version], text)
            return [tm.span(s, e) for s, e in spans]

        diags: List[Dict[str, Any]] = []
        if doc.diags:
            version, items = doc.diags
            moved = carried(version, [(d["start"], d["end"]) for d in items])
            diags = [dict(d, start=m[0], end=m[1]) for d, m in zip(items, moved) if m]
        processing: Optional[List[Tuple[int, int]]] = None
        if doc.progress:
            version, ranges = doc.progress
            processing = [m for m in carried(version, ranges) if m]
            if version != seq:
                processing = None
        rep = build_report(sp.commands, diags, processing)
        doc.report, doc.report_seq = rep, seq
        colours: List[List[Any]] = lexical_colours(sp.mask)
        if doc.colours:
            version, items = doc.colours
            if version == seq:
                colours += items
            elif version in doc.history:
                tm = TextMap(doc.history[version], text)
                for s, e, k in items:
                    m = tm.span(s, e)
                    if m:
                        colours.append([m[0], m[1], k])
        colours.sort(key=lambda c: (c[0], c[1]))
        theory = self.module_name(doc.path)
        header_errors = [STALE_HEADER_ERROR] if doc.stale else []
        return {
            "event": "node", "file": doc.path, "theory": theory, "seq": seq, "status": rep["status"],
            "header_errors": header_errors, "imports": self.imports_of(text), "commands": rep["commands"],
            "messages": [diag_message(d) for d in diags], "colours": colours,
        }

    def module_name(self, path: str) -> str:
        with contextlib.suppress(ValueError):
            rel = os.path.relpath(path, self.lake_root)
            if not rel.startswith(".."):
                return rel[:-5].replace(os.sep, ".") if rel.endswith(".lean") else rel
        return os.path.splitext(os.path.basename(path))[0]

    def imports_of(self, text: str) -> List[str]:
        """Paths of the local modules a file imports"""
        out = []
        for m in re.finditer(r"^import\s+([\w.«»]+)", text, re.M):
            rel = m.group(1).replace(".", os.sep) + ".lean"
            p = os.path.join(self.lake_root, rel)
            if os.path.isfile(p):
                out.append(p)
        return out

    # ----- colours

    def _schedule_colours(self, doc: LDoc) -> None:
        if doc.colour_task and not doc.colour_task.done():
            return
        doc.colour_task = asyncio.get_running_loop().create_task(self._fetch_colours(doc))

    async def _fetch_colours(self, doc: LDoc) -> None:
        await asyncio.sleep(0.2)
        version = doc.version
        if not (self.client and doc.server_open and self.legend):
            return
        try:
            res = await self.client.request("textDocument/semanticTokens/full",
                                            {"textDocument": {"uri": doc.uri}}, timeout=60)
        except (LspError, LspClosed, asyncio.TimeoutError):
            return
        if not res or version != doc.version or not doc.server_open:
            return
        doc.colours = (version, decode_tokens(res.get("data", []), self.legend, doc.splitter(version).index))
        self._schedule_node(doc)

    # ----- state

    def command_at(self, doc: LDoc, offset: int) -> Optional[Tuple[int, int, str]]:
        """The command at the offset; the space after a command belongs to it"""
        cmds = doc.splitter().commands
        i = bisect.bisect_right([c[0] for c in cmds], offset) - 1
        return cmds[i] if i >= 0 else None

    def status_of(self, doc: LDoc, cmd: Tuple[int, int, str]) -> str:
        rep = doc.report
        if rep and doc.report_seq == doc.version:
            for s, e, n, st in rep["commands"]:
                if s == cmd[0] and e == cmd[1]:
                    return st
        return "unprocessed"

    async def _req_state(self, obj: Dict[str, Any]) -> Dict[str, Any]:
        doc = self.docs.get(obj["file"])
        if doc is None:
            return {"error": "That file is not open"}
        offset = int(obj.get("offset", 0))
        cmd = self.command_at(doc, offset)
        outdated = not doc.diags or doc.diags[0] < doc.version
        if cmd is None:
            return {"command": None, "messages": [], "outdated": outdated}
        msgs = []
        cmds = doc.splitter().commands
        if doc.diags and doc.diags[0] in doc.history:
            tm = TextMap(doc.history[doc.diags[0]], doc.text)
            for d in doc.diags[1]:
                sp = tm.span(d["start"], d["end"])
                if sp and cmds[anchor_command(cmds, sp[0]) or 0] == cmd:
                    msgs.append(diag_message(dict(d, start=sp[0], end=sp[1])))
        state = await self._plain_state(doc, offset)
        if state:
            msgs.append(message("state", state))
        return {"command": {"start": cmd[0], "end": cmd[1], "name": cmd[2], "status": self.status_of(doc, cmd)},
                "messages": msgs, "outdated": outdated}

    async def _plain_state(self, doc: LDoc, offset: int) -> str:
        if not (self.client and doc.server_open):
            return ""
        pos = {"textDocument": {"uri": doc.uri}, "position": doc.splitter().index.to_lsp(offset)}
        try:
            res = await self.client.request("$/lean/plainGoal", pos, timeout=30)
            if res is not None:
                goals = res.get("goals") or []
                return "\n\n".join(goals) if goals else "No goals"
            res = await self.client.request("$/lean/plainTermGoal", pos, timeout=30)
            return (res or {}).get("goal", "") if res else ""
        except (LspError, LspClosed, asyncio.TimeoutError):
            return ""

    # ----- info

    async def _req_info(self, obj: Dict[str, Any]) -> Dict[str, Any]:
        doc = self.docs.get(obj["file"])
        if doc is None or not self.client or not doc.server_open:
            return {"info": {"tooltip": None, "definition": None}}
        index = doc.splitter().index
        params = {"textDocument": {"uri": doc.uri}, "position": index.to_lsp(int(obj.get("offset", 0)))}
        tooltip = definition = None
        with contextlib.suppress(LspError, asyncio.TimeoutError):
            hover = await self.client.request("textDocument/hover", params, timeout=30)
            if hover and hover.get("contents"):
                contents = hover["contents"]
                value = contents.get("value", "") if isinstance(contents, dict) else \
                    "\n".join(c if isinstance(c, str) else c.get("value", "") for c in contents) \
                    if isinstance(contents, list) else str(contents)
                s, e = index.range_to_offsets(hover["range"]) if hover.get("range") else \
                    (int(obj.get("offset", 0)), int(obj.get("offset", 0)) + 1)
                items = tooltip_items(value)
                if items:
                    tooltip = {"start": s, "end": e, "items": [
                        {"kind": "writeln", "raw": t, "text": t, "spans": []} for t in items]}
        with contextlib.suppress(LspError, asyncio.TimeoutError):
            locs = await self.client.request("textDocument/definition", params, timeout=30)
            definition = self._definition(locs)
        return {"info": {"tooltip": tooltip, "definition": definition}}

    def _definition(self, locs: Any) -> Optional[Dict[str, Any]]:
        if isinstance(locs, dict):
            locs = [locs]
        if not locs:
            return None
        loc = locs[0]
        uri = loc.get("targetUri") or loc.get("uri")
        rng = loc.get("targetSelectionRange") or loc.get("range")
        if not uri or not rng:
            return None
        path = urllib.parse.unquote(urllib.parse.urlparse(uri).path)
        target = self.docs.get(path)
        text = target.text if target else read_text(path)
        if text is None:
            return None
        text = text.replace("\r\n", "\n")
        index = TextIndex(text)
        s, e = index.range_to_offsets(rng)
        line = index.line_of(s)
        top = next((t for t in Splitter(text).toplevel if t[0] <= s < t[1]), None)
        kind = top[2] if top and top[2] in LEAN_KINDS else "definition"
        return {"file": path, "line": line + 1, "column": s - index.starts[line], "start": s, "end": e,
                "name": text[s:e], "kind": kind}

    # ----- queries

    def _send_query(self, obj: Dict[str, Any]) -> None:
        qid = obj["query"]
        task = asyncio.get_running_loop().create_task(self._run_query(obj))
        self.queries[qid] = task
        task.add_done_callback(lambda _t, q=qid: self.queries.pop(q, None))

    def _query_event(self, qid: str, status: str, messages: Optional[List[Dict[str, Any]]] = None, **extra: Any) -> None:
        self.emit(dict({"event": "query", "query": qid, "status": status, "messages": messages or []}, **extra))

    async def _run_query(self, obj: Dict[str, Any]) -> None:
        qid, op = obj["query"], obj.get("operation")
        try:
            doc = self.docs.get(obj.get("file", ""))
            if op == "loogle":
                return await self._query_loogle(qid, obj.get("args") or [])
            if op not in ("deductum_goals", "deductum_command"):
                return self._query_event(qid, "failed", error=f"Unknown query operation: {op}")
            if doc is None:
                return self._query_event(qid, "failed", error="That file is not open")
            if not self.client or not doc.server_open:
                return self._query_event(qid, "failed", error="Checking is paused")
            if op == "deductum_goals":
                await self._query_goals(qid, doc, int(obj.get("offset", 0)))
            else:
                await self._query_command(qid, doc, int(obj.get("offset", 0)), (obj.get("args") or [""])[0])
        except asyncio.CancelledError:
            self._query_event(qid, "canceled")
        except (LspClosed, LspError) as exn:
            self._query_event(qid, "failed", error=f"Lean server: {getattr(exn, 'message', exn)}")
        except Exception as exn:  # a bug here must not leave the query waiting
            self._query_event(qid, "failed", error=f"{type(exn).__name__}: {exn}")

    async def _query_loogle(self, qid: str, args: List[str]) -> None:
        """Search by name or type pattern: ``args`` are the query and the source (``public``, ``local``
        or ``off``; public when left out). Needs no open file."""
        query = args[0] if args else ""
        source = (args[1] if len(args) > 1 else "public") or "public"
        self._query_event(qid, "waiting")
        self._query_event(qid, "running")
        try:
            result = await asyncio.get_running_loop().run_in_executor(None, loogle_fetch, query, source)
        except HostError as exn:
            return self._query_event(qid, "failed", error=str(exn))
        self._query_event(qid, "finished", loogle_messages(result, source), source=source)

    def _command_fields(self, doc: LDoc, offset: int) -> Dict[str, Any]:
        cmd = self.command_at(doc, offset)
        if not cmd:
            return {}
        fields: Dict[str, Any] = {"start": cmd[0], "end": cmd[1],
                                  "command": {"start": cmd[0], "end": cmd[1], "name": cmd[2]}}
        if cmd[2] in self.placeholder_commands:
            fields["replace"] = {"start": cmd[0], "end": cmd[1]}
        return fields

    async def _query_goals(self, qid: str, doc: LDoc, offset: int) -> None:
        fields = self._command_fields(doc, offset)
        self._query_event(qid, "waiting", **fields)
        self._query_event(qid, "running", **fields)
        # A placeholder asks for the goal it stands for: the one before it
        pos = fields["start"] if fields.get("replace") else offset
        goals: Optional[List[Dict[str, Any]]] = None
        nogoal = False
        try:
            goals = await self._interactive_goals(doc, pos)
        except (LspError, LspClosed, asyncio.TimeoutError, KeyError, TypeError):
            goals = None
        pos_lsp = {"textDocument": {"uri": doc.uri}, "position": doc.splitter().index.to_lsp(pos)}
        if goals is None or not goals:
            plain = await self.client.request("$/lean/plainGoal", pos_lsp, timeout=60) if self.client else None
            if plain is not None:
                goals = [parse_goal_text(g) for g in plain.get("goals") or []]
                nogoal = not goals
            else:
                term = await self.client.request("$/lean/plainTermGoal", pos_lsp, timeout=60) if self.client else None
                goals = [parse_goal_text(term["goal"])] if term and term.get("goal") else []
                if not goals:
                    return self._query_event(qid, "failed", error="Not inside a proof", **fields)
        self._query_event(qid, "finished", goal_parts(goals, nogoal), **fields)

    async def _interactive_goals(self, doc: LDoc, offset: int) -> List[Dict[str, Any]]:
        pos = doc.splitter().index.to_lsp(offset)
        for attempt in (0, 1):
            session = await self._rpc_session(doc)
            try:
                res = await self.client.request("$/lean/rpc/call", {
                    "sessionId": session, "method": "Lean.Widget.getInteractiveGoals",
                    "textDocument": {"uri": doc.uri}, "position": pos,
                    "params": {"textDocument": {"uri": doc.uri}, "position": pos}}, timeout=60)
                return [goal_from_interactive(g) for g in (res or {}).get("goals", [])]
            except LspError as exn:
                if attempt == 0 and ("session" in exn.message.lower() or exn.code == -32900):
                    doc.rpc = None
                    continue
                raise
        return []

    async def _rpc_session(self, doc: LDoc) -> str:
        """The document's RPC session, kept alive while the document is open (it is reconnected
        after the document is reopened, as the server drops the session then)"""
        assert self.client
        if doc.rpc:
            return doc.rpc
        res = await self.client.request("$/lean/rpc/connect", {"uri": doc.uri}, timeout=60)
        doc.rpc = res["sessionId"]
        if not doc.rpc_task or doc.rpc_task.done():
            doc.rpc_task = asyncio.get_running_loop().create_task(self._keep_alive(doc))
        return doc.rpc

    async def _keep_alive(self, doc: LDoc) -> None:
        while self.client and doc.server_open and doc.rpc:
            await asyncio.sleep(self.KEEPALIVE)
            if doc.rpc:
                self._notify("$/lean/rpc/keepAlive", {"uri": doc.uri, "sessionId": doc.rpc})

    # ----- checking a candidate in a scratch document

    def scratch_text(self, doc: LDoc, offset: int, text: str) -> Optional[Tuple[str, int, int, bool]]:
        """The text to check and the range in it to look for errors in: the file up to the end of the
        declaration around ``offset`` with the command there replaced by ``text``, or, for a command
        text such as ``#check``, with it on a new line after the declaration. The last item says
        which of the two it is."""
        sp = doc.splitter()
        top = next((t for t in sp.toplevel if t[0] <= offset <= t[1]), None)
        if top is None:
            earlier = [t for t in sp.toplevel if t[0] <= offset]
            top = earlier[-1] if earlier else None
        if top is None:
            return None
        ds, de, _ = top
        src = doc.text
        first = text.lstrip().split(None, 1)[0] if text.strip() else ""
        if first.startswith("#") or first in LEAN_KINDS and LEAN_KINDS[first] in ("thy_goal_stmt", "thy_decl"):
            body = src[:de] + "\n" + text.strip("\n") + "\n"
            return body, de + 1, len(body), True
        cmd = self.command_at(doc, offset)
        if cmd is None or not (ds <= cmd[0] < de):
            return None
        ls = src.rfind("\n", 0, cmd[0]) + 1
        before = src[ls:cmd[0]]
        indent = before if not before.strip() else " " * len(before)
        lines = text.strip("\n").split("\n")
        new = lines[0] + "".join("\n" + (indent + ln if ln.strip() else ln) for ln in lines[1:])
        body = src[:cmd[0]] + new + src[cmd[1]:de]
        return body, ds, len(body), False

    async def _query_command(self, qid: str, doc: LDoc, offset: int, text: str) -> None:
        fields = self._command_fields(doc, offset)
        self._query_event(qid, "waiting", **fields)
        plan = self.scratch_text(doc, offset, text)
        if plan is None:
            return self._query_event(qid, "failed", error="There is no command at that position", **fields)
        body, lo, hi, as_command = plan
        assert self.check_slots is not None and self.client is not None
        async with self.check_slots:
            self._query_event(qid, "running", **fields)
            uri = Path(os.path.join(self.lake_root, f".deductum-check-{uuid.uuid4().hex[:8]}.lean")).as_uri()
            sc = self.scratch[uri] = Scratch(uri, body)
            try:
                self._notify("textDocument/didOpen", {"textDocument": {
                    "uri": uri, "languageId": "lean4", "version": 1, "text": body}})
                try:
                    await asyncio.wait_for(sc.done.wait(), 110)
                except asyncio.TimeoutError:
                    return self._query_event(qid, "failed", error="Lean did not finish checking in time", **fields)
                msgs = []
                for d in sc.diags:
                    s, e = sc.index.range_to_offsets(d["range"])
                    sev = d.get("severity", 1)
                    if not (lo <= s <= hi):
                        continue
                    if sev == 1:
                        msgs.append(message("error", d.get("message", "")))
                    elif as_command:
                        msgs.append(message(KINDS_BY_SEVERITY.get(sev, "writeln"), d.get("message", "")))
                self._query_event(qid, "finished", msgs, **fields)
            finally:
                self._notify("textDocument/didClose", {"textDocument": {"uri": uri}})
                self.scratch.pop(uri, None)
