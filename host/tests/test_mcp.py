import os
import sys
import unittest
from unittest import mock

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

import deductum_host as H  # noqa: E402
import deductum_mcp as M  # noqa: E402

TEXT = "theory T imports Main begin\nlemma foo: \"a = a\"\n  sorry\nend\n"


class FakeClient:
    """Stands in for H.SyncClient: records requests, answers from a canned session"""

    def __init__(self, summary=None):
        self.summary = dict({"id": "s1", "name": "proj", "folder": "/p", "lifecycle": "Running", "logic": "HOL",
                             "epoch": 1, "theories": [], "grants": [], "clients": []}, **(summary or {}))
        self.calls = []

    def request(self, op, **p):
        self.calls.append((op, p))
        if op == "hello":
            return {"key": "k"}
        if op in ("session_info", "session_create"):
            return {"session": self.summary}
        if op == "sessions":
            return {"sessions": [dict(self.summary, grants=[], clients=[])]}
        if op == "read":
            return {"doc": {"path": p["path"], "revision": 3, "text": TEXT, "dirty": False, "conflict": False,
                            "prover": {"messages": []}}}
        if op == "query":
            return {"query": "q1"}
        if op == "query_status":
            return {"query": {"status": "finished", "messages": [{"kind": "writeln", "text": "output here"}]}}
        if op == "notices":
            return {"notices": []}
        raise AssertionError(f"unexpected op {op}")

    def queries(self):
        return [p for op, p in self.calls if op == "query"]


def adapter(summary=None):
    with mock.patch.object(M, "identity_from_env", return_value=None):
        a = M.Adapter(FakeClient(summary))
    a.poll = 0
    return a


def call(a, name, **args):
    return a.call(name, args)


POS = dict(session="s1", path="/p/T.thy", line=2)


class ToolList(unittest.TestCase):
    def test_names(self):
        with mock.patch.dict(os.environ, {}, clear=False):
            os.environ.pop("DEDUCTUM_MCP_ALIASES", None)
            names = [t["name"] for t in M.listed_tools()]
        self.assertTrue(all(n.startswith("proof_") for n in names))
        self.assertEqual(len(names), len(set(names)))
        for n in ("proof_sessions", "proof_open_session", "proof_run_tool", "proof_search", "proof_command",
                  "proof_capabilities", "proof_read", "proof_edit", "proof_propose", "proof_listen"):
            self.assertIn(n, names)
        self.assertEqual(len(names), 29)
        self.assertEqual(M.SERVER_INFO["name"], "deductum")

    def test_aliases_listed_on_request(self):
        with mock.patch.dict(os.environ, {"DEDUCTUM_MCP_ALIASES": "1"}):
            names = [t["name"] for t in M.listed_tools()]
        self.assertIn("isabelle_sledgehammer", names)
        self.assertIn("proof_sessions", names)

    def test_aliases_map_to_tools(self):
        names = {t["name"] for t in M.TOOLS}
        for old, new in M.ALIASES.items():
            self.assertIn(new, names, old)


class Calls(unittest.TestCase):
    def test_alias_call_same_handler(self):
        a = adapter()
        self.assertEqual(call(a, "isabelle_sessions"), call(a, "proof_sessions"))
        self.assertIn("backend isabelle", call(a, "proof_sessions"))
        self.assertEqual(call(a, "isabelle_session_info", session="s1"), call(a, "proof_session_info", session="s1"))

    def test_capabilities_default_isabelle(self):
        out = call(adapter(), "proof_capabilities", session="s1")
        self.assertIn("backend: isabelle", out)
        self.assertIn("sledgehammer", out)

    def test_capabilities_from_summary(self):
        a = adapter({"backend": "lean", "toolchain": {"name": "leanprover/lean4:v4.12"},
                     "capabilities": {"tools": [{"name": "exact?"}, {"name": "apply?"}],
                                      "search": {"label": "Loogle", "operation": "loogle"},
                                      "command": ["#check", "#eval"]}})
        out = call(a, "proof_capabilities", session="s1")
        self.assertIn("backend: lean", out)
        self.assertIn("exact?, apply?", out)
        self.assertIn("v4.12", out)
        self.assertIn("#check, #eval", out)

    def test_run_tool_refused(self):
        a = adapter({"backend": "lean", "capabilities": {"tools": [{"name": "exact?"}]}})
        with self.assertRaises(H.HostError) as cm:
            call(a, "proof_run_tool", tool="sledgehammer", **POS)
        self.assertIn("exact?", str(cm.exception))
        self.assertEqual(a.client.queries(), [])

    def test_search_refused_without_capability(self):
        a = adapter({"backend": "lean", "capabilities": {"tools": []}})
        with self.assertRaises(H.HostError):
            call(a, "proof_search", query="x", **POS)

    def test_command_refused(self):
        a = adapter()
        with self.assertRaises(H.HostError) as cm:
            call(a, "proof_command", text="ML \"1\"", **POS)
        self.assertIn("quickcheck", str(cm.exception))

    def test_run_tool_isabelle_defaults(self):
        a = adapter()
        call(a, "proof_run_tool", tool="sledgehammer", **POS)
        (q,) = a.client.queries()
        self.assertEqual(q["operation"], "sledgehammer")
        self.assertEqual(q["args"], ["", "false", "true"])
        self.assertEqual(q["revision"], 3)
        self.assertEqual(q["offset"], TEXT.index("lemma foo: \"a = a\"") + len("lemma foo: \"a = a\"") - 1)

    def test_run_tool_args_and_offset(self):
        a = adapter()
        call(a, "proof_run_tool", session="s1", path="/p/T.thy", offset=5, tool="sledgehammer",
             args={"provers": "cvc5 z3", "isar_proofs": True, "try0": False})
        (q,) = a.client.queries()
        self.assertEqual((q["offset"], q["args"]), (5, ["cvc5 z3", "true", "false"]))

    def test_sledgehammer_alias(self):
        a = adapter()
        call(a, "isabelle_sledgehammer", provers="z3", isar_proofs=True, **POS)
        (q,) = a.client.queries()
        self.assertEqual((q["operation"], q["args"]), ("sledgehammer", ["z3", "true", "true"]))

    def test_other_isabelle_tool_is_a_command(self):
        a = adapter()
        call(a, "proof_run_tool", tool="try0", **POS)
        (q,) = a.client.queries()
        self.assertEqual((q["operation"], q["args"]), ("deductum_command", ["try0"]))

    def test_run_tool_needs_position(self):
        with self.assertRaises(H.HostError):
            call(adapter(), "proof_run_tool", session="s1", path="/p/T.thy", tool="sledgehammer")

    def test_search_isabelle(self):
        a = adapter()
        self.assertIn("output here", call(a, "isabelle_find_theorems", query="name: comm", **POS))
        (q,) = a.client.queries()
        self.assertEqual((q["operation"], q["args"]), ("find_theorems", ["40", "false", "name: comm"]))

    def test_search_uses_capability_operation(self):
        a = adapter({"backend": "lean", "capabilities": {"search": {"operation": "lean_search"}}})
        call(a, "proof_search", query="Nat.add_comm", **POS)
        (q,) = a.client.queries()
        self.assertEqual((q["operation"], q["args"]), ("lean_search", ["Nat.add_comm"]))

    def test_command_routing(self):
        a = adapter()
        out = call(a, "proof_command", text="value \"1+1::nat\"", **POS)
        (q,) = a.client.queries()
        self.assertEqual((q["operation"], q["args"]), ("deductum_command", ["value \"1+1::nat\""]))
        self.assertIn("output here", out)
        call(a, "isabelle_run_diagnostic", command="quickcheck", **POS)
        self.assertEqual(a.client.queries()[-1]["args"], ["quickcheck"])

    def test_command_lean(self):
        a = adapter({"backend": "lean", "capabilities": {"command": ["#check"]}})
        call(a, "proof_command", text="#check Nat", **POS)
        self.assertEqual(a.client.queries()[0]["args"], ["#check Nat"])

    def test_open_session_passes_backend(self):
        a = adapter()
        call(a, "proof_open_session", folder="/p", backend="lean", toolchain="stable", project={"k": 1})
        op, p = [c for c in a.client.calls if c[0] == "session_create"][0]
        self.assertEqual((p["backend"], p["toolchain"], p["project"]), ("lean", "stable", {"k": 1}))
        call(a, "proof_open_session", folder="/p")
        p = [c for c in a.client.calls if c[0] == "session_create"][1][1]
        self.assertNotIn("backend", p)


if __name__ == "__main__":
    unittest.main()
