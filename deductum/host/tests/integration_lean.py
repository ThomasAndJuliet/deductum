#!/usr/bin/env python3
"""Drives LeanDriver against a real ``lake serve`` (needs elan with the project's toolchain).

    python3 integration_lean.py [--dsrv PATH] [--skip-dsrv] [--skip-tiny] [--keep]

(a) A tiny Lake project generated in a scratch directory. (b) A scratch copy of DSRVLean, the
user's main Lean project: fetched with ``scp -r d59500:~/Documents/Research/Repositories/DSRVLean``
into ``~/tmp-deductum-lean/DSRVLean`` unless ``--dsrv`` names a copy. The originals are never
touched. Prints PASS or FAIL for each check; the exit status is the number of failures.
"""
import argparse
import asyncio
import contextlib
import os
import shutil
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, ".."))
from drivers.lean import LeanDriver, STALE_HEADER_ERROR, tool_env  # noqa: E402

SCRATCH = os.path.expanduser("~/tmp-deductum-lean")
FAILURES = 0


def check(name, ok, detail=""):
    global FAILURES
    print(("PASS " if ok else "FAIL ") + name + ((" -- " + str(detail)[:300]) if detail and not ok else ""), flush=True)
    if not ok:
        FAILURES += 1


class FakeSession:
    """Just what the driver needs of the host's session: ``meta`` and ``on_backend_event``"""

    def __init__(self, folder):
        self.meta = {"folder": folder}
        self.events = []

    def on_backend_event(self, obj):
        self.events.append(obj)

    async def wait(self, pred, timeout=120.0, start=0):
        """The first event from index ``start`` that satisfies ``pred``"""
        t0 = time.monotonic()
        seen = start
        while time.monotonic() - t0 < timeout:
            while seen < len(self.events):
                ev = self.events[seen]
                seen += 1
                if pred(ev):
                    return ev
            await asyncio.sleep(0.05)
        return None

    def nodes(self, path):
        return [e for e in self.events if e.get("event") == "node" and e["file"] == path]


class Harness:
    def __init__(self, root):
        self.root = root
        self.session = FakeSession(root)
        self.drv = LeanDriver(self.session)
        self.rev = {}
        self.text = {}

    async def start(self):
        t0 = time.monotonic()
        await self.drv.start()
        ready = await self.session.wait(lambda e: e.get("event") in ("ready", "phase"), 600)
        return ready, time.monotonic() - t0

    def open(self, path, text=None):
        text = open(path, encoding="utf-8").read() if text is None else text
        self.text[path] = text
        self.rev[path] = 1
        self.drv.send({"op": "open", "file": path, "text": text, "rev": 1})
        return text

    def edit(self, path, edits):
        """Sequential edits ``(start, end, text)``; returns the new revision"""
        for s, e, t in edits:
            self.text[path] = self.text[path][:s] + t + self.text[path][e:]
        self.rev[path] += 1
        self.drv.send({"op": "edit", "file": path, "rev": self.rev[path],
                       "edits": [{"start": s, "end": e, "text": t} for s, e, t in edits]})
        return self.rev[path]

    async def settled(self, path, rev=None, timeout=300.0):
        """The first node that covers revision ``rev`` and says the file is fully checked"""
        rev = self.rev[path] if rev is None else rev
        start = 0
        t0 = time.monotonic()
        while time.monotonic() - t0 < timeout:
            for n in self.session.nodes(path)[start:]:
                if n["seq"] == rev and n["status"]["consolidated"] and n["status"]["running"] == 0:
                    return n
            start = len(self.session.nodes(path))
            await asyncio.sleep(0.1)
        return None

    async def query(self, op, path, offset, args=None, timeout=300.0):
        self.counter = getattr(self, "counter", 0) + 1
        qid = "q%d" % self.counter
        first = len(self.session.events)
        self.drv.send({"op": "query", "query": qid, "file": path, "offset": offset, "operation": op,
                       "args": args or []})
        done = await self.session.wait(lambda e: e.get("event") == "query" and e["query"] == qid
                                       and e["status"] in ("finished", "failed", "canceled"), timeout, first)
        evs = [e for e in self.session.events[first:] if e.get("event") == "query" and e["query"] == qid]
        return done, evs


def names(node):
    return [c[2] for c in node["commands"]]


def status_of(node, text, needle, name=None):
    off = text.index(needle)
    for s, e, n, st in node["commands"]:
        if s <= off < max(e, s + 1) and (name is None or n == name):
            return st
    return None


# ---------------------------------------------------------------------------
# (a) the generated project

TINY_MAIN = """/-! # Tiny -/

/-- Commutativity. -/
theorem add_comm' (a b : Nat) : a + b = b + a := by
  induction a with
  | zero => simp
  | succ n ih =>
    omega

theorem and_intro' (p q : Prop) (hp : p) (hq : q) : p ∧ q := by
  constructor
  · exact hp
  · sorry

def double (n : Nat) : Nat := n + n

example : double 2 = 4 := by
  decide

theorem uses_it (a b : Nat) : a + b = b + a := add_comm' a b
"""


def make_tiny():
    root = tempfile.mkdtemp(prefix="tiny-", dir=SCRATCH)
    os.makedirs(os.path.join(root, "Tiny"))
    with open(os.path.join(root, "lakefile.toml"), "w") as f:
        f.write('name = "tiny"\nversion = "0.1.0"\ndefaultTargets = ["Tiny"]\n\n[[lean_lib]]\nname = "Tiny"\n')
    with open(os.path.join(root, "lean-toolchain"), "w") as f:
        f.write("leanprover/lean4:v4.19.0\n")
    with open(os.path.join(root, "Tiny.lean"), "w") as f:
        f.write("import Tiny.Main\nimport Tiny.UseDep\n")
    with open(os.path.join(root, "Tiny", "Main.lean"), "w") as f:
        f.write(TINY_MAIN)
    with open(os.path.join(root, "Tiny", "Dep.lean"), "w") as f:
        f.write("def depValue : Nat := 1\n")
    with open(os.path.join(root, "Tiny", "UseDep.lean"), "w") as f:
        f.write("import Tiny.Dep\n\n#eval depValue\n")
    return root


def goal_kinds(messages):
    out = {}
    for m in messages:
        head, _, body = m["raw"].partition("\n")
        kind, _, n = head[len("#deductum:"):].partition(" ")
        out.setdefault(kind, []).append((int(n or 0), body))
    return out


async def run_tiny():
    print("== tiny project", flush=True)
    root = make_tiny()
    main = os.path.join(root, "Tiny", "Main.lean")
    det = LeanDriver.detect(os.path.join(root, "Tiny"))
    check("detect finds the Lake root", bool(det) and det["project"]["lake_root"] == root, det)
    check("detect reads the toolchain", bool(det) and det["toolchain"] == "leanprover/lean4:v4.19.0", det)
    tcs = LeanDriver.toolchains()
    check("toolchains lists v4.19.0", any(t["version"] == "4.19.0" for t in tcs), tcs)

    h = Harness(root)
    drv, sess = h.drv, h.session
    ready, took = await h.start()
    check("ready event", bool(ready) and ready["event"] == "ready" and drv.ready, ready)
    if not ready or ready["event"] != "ready":
        return
    check("ready identifier and home", ready["identifier"].startswith("Lean 4.19") and ready["home"] == root, ready)
    caps = drv.capabilities()
    check("capabilities: lexical spans, scratch checks, tools",
          caps["spans"] == "lexical" and caps["check_candidates"] == "scratch"
          and "exact?" in [t["name"] for t in caps["tools"]] and caps["goal_style"] == "lean", caps)
    kinds = (await drv.request({"op": "syntax"}))["syntax"]["kinds"]
    check("syntax kinds", kinds["theorem"] == "thy_goal_stmt" and kinds["def"] == "thy_decl"
          and kinds["/-!"] == "document_body" and kinds["simp"] == "prf_script", {k: kinds.get(k) for k in ("theorem", "def")})
    check("ping and symbols", await drv.request({"op": "ping"}) == {} and (await drv.request({"op": "symbols"}))["symbols"] == [])

    # ----- open and check
    text = h.open(main)
    first = await sess.wait(lambda e: e.get("event") == "node" and e["file"] == main, 30)
    check("a node arrives soon after open", first is not None)
    node = await h.settled(main)
    check("the file is checked (consolidated)", node is not None)
    if node is None:
        return
    check("command names", names(node)[:6] == ["/-!", "theorem", "induction", "|", "simp", "|"], names(node)[:8])
    check("sorry is a command of its own", "sorry" in names(node), names(node))
    check("commands tile in order without overlap",
          all(a[1] <= b[0] and a[0] < a[1] for a, b in zip(node["commands"], node["commands"][1:])))
    check("a theorem with sorry is warned", status_of(node, text, "theorem and_intro'") == "warned",
          status_of(node, text, "theorem and_intro'"))
    check("proof tactics are finished", status_of(node, text, "omega") == "finished")
    check("status counts", node["status"]["total"] == len(node["commands"]) and node["status"]["percentage"] == 100
          and node["status"]["warned"] >= 1 and node["status"]["failed"] == 0, node["status"])
    warn = [m for m in node["messages"] if m["kind"] == "warning"]
    check("the sorry warning is a message at the declaration name",
          any("sorry" in m["text"] and text[m["start"]:m["end"]] == "and_intro'" for m in warn), node["messages"])
    cn = await sess.wait(lambda e: e.get("event") == "node" and e["file"] == main
                         and any(c[2] == "keyword1" for c in e["colours"]), 30)
    classes = {c[2] for c in (cn or node)["colours"]}
    check("colours: keywords, variables, comments", {"keyword1", "free", "comment1"} <= classes, classes)
    check("theory name", node["theory"] == "Tiny.Main", node["theory"])

    # ----- goals
    done, evs = await h.query("deductum_goals", main, text.index("omega"))
    check("goals: waiting, running, finished", [e["status"] for e in evs][:2] == ["waiting", "running"]
          and done and done["status"] == "finished", [e["status"] for e in evs])
    if done and done["status"] == "finished":
        parts = goal_kinds(done["messages"])
        check("goals at omega: count, param ih, concl", parts["count"][0][0] == 1 and any(b.startswith("ih : ") for _, b in parts["param"])
              and "n + 1" in parts["concl"][0][1], parts)
        check("goals at omega: case tag", parts.get("case") == [(1, "succ")], parts.get("case"))
        check("goals at omega: text as the infoview", parts["text"][0][1].startswith("case succ\n") and "⊢" in parts["text"][0][1], parts["text"])
        check("goal command fields", done["command"]["name"] == "omega" and done["start"] == text.index("omega"), done)
    sorry_at = text.index("sorry")
    done, evs = await h.query("deductum_goals", main, sorry_at)
    check("goals at sorry: replace range is the sorry", bool(done) and done.get("replace") == {"start": sorry_at, "end": sorry_at + 5}, done and done.get("replace"))
    if done and done["status"] == "finished":
        parts = goal_kinds(done["messages"])
        check("goals at sorry: the goal before it", parts["case"] == [(1, "right")] and parts["concl"][0][1] == "q", parts)
        check("hypotheses as bundles", ("p q : Prop" in [b for _, b in parts["param"]]), parts["param"])
    done, _ = await h.query("deductum_goals", main, text.index("omega") + 1)
    check("no goals after the last tactic", bool(done) and "nogoal" in goal_kinds(done["messages"]), done and done["messages"])
    done, _ = await h.query("deductum_goals", main, text.index("n + n :=") if "n + n :=" in text else text.index("n + n"))
    if done and done["status"] == "finished":
        parts = goal_kinds(done["messages"])
        check("term goal outside tactic mode", parts["concl"][0][1] == "Nat" and parts["count"][0][0] == 1, parts)
    else:
        check("term goal outside tactic mode", False, done)
    done, _ = await h.query("nonsense", main, 0)
    check("unknown query operation fails naming it", bool(done) and done["status"] == "failed" and "nonsense" in done["error"], done)
    r = await drv.request({"op": "state", "file": main, "offset": sorry_at, "margin": 80})
    check("state at sorry", r["command"]["name"] == "sorry" and r["command"]["status"] in ("finished", "warned")
          and any(m["kind"] == "state" and "case right" in m["text"] for m in r["messages"]) and not r["outdated"], r)
    r = await drv.request({"op": "state", "file": main, "offset": text.index("theorem and_intro'")})
    check("state at a declaration shows its warning", any(m["kind"] == "warning" for m in r["messages"]), r["messages"])

    # ----- candidates in a scratch document
    done, evs = await h.query("deductum_command", main, sorry_at, ["exact hq"])
    check("deductum_command accepts a correct tactic", bool(done) and done["status"] == "finished"
          and not [m for m in done["messages"] if m["kind"] == "error"], done)
    done, evs = await h.query("deductum_command", main, sorry_at, ["exact hp"])
    check("deductum_command rejects a wrong tactic", bool(done) and done["status"] == "finished"
          and any(m["kind"] == "error" for m in done["messages"]), done)
    done, _ = await h.query("deductum_command", main, sorry_at, ["constructor\n  all_goals trivial"])
    check("multi-line candidate is re-indented", bool(done) and done["status"] == "finished", done)
    done, _ = await h.query("deductum_command", main, sorry_at, ["#check @and_intro'"])
    check("#check goes after the declaration and returns its output", bool(done) and done["status"] == "finished"
          and any("and_intro'" in m["text"] and m["kind"] == "information" for m in done["messages"]), done)
    fact = drv.fact_check_text(["and_intro'", "Nat.add_comm"])
    check("fact_check_text", fact == "#check @and_intro'\n#check @Nat.add_comm", fact)
    done, _ = await h.query("deductum_command", main, sorry_at, [fact])
    check("existing facts check cleanly", bool(done) and not [m for m in done["messages"] if m["kind"] == "error"], done)
    done, _ = await h.query("deductum_command", main, sorry_at, [drv.fact_check_text(["no_such_fact"])])
    check("a missing fact is an error", bool(done) and any(m["kind"] == "error" for m in done["messages"]), done)
    check("the real file is untouched by checks", h.text[main] == text and not any(
        f.startswith(".deductum-check") for f in os.listdir(root)))
    # two at once
    qs = await asyncio.gather(*[h.query("deductum_command", main, sorry_at, [t]) for t in ("exact hq", "exact hp", "assumption")])
    check("parallel checks", [bool(d) and any(m["kind"] == "error" for m in d["messages"]) for d, _ in qs] == [False, True, False],
          [d and d["messages"] for d, _ in qs])

    # ----- an edit with a type error, then a sorry
    rev = h.edit(main, [(text.index("omega"), text.index("omega") + 5, "exact foo_bar")])
    r = await drv.request({"op": "state", "file": main, "offset": h.text[main].index("exact foo_bar")})
    check("state is outdated right after an edit", r["outdated"] is True, r)
    node = await h.settled(main, rev)
    check("diagnostics for the edit carry its revision", node is not None and node["seq"] == rev)
    if node:
        t2 = h.text[main]
        check("the edited tactic failed", status_of(node, t2, "exact foo_bar") == "failed", status_of(node, t2, "exact foo_bar"))
        errs = [m for m in node["messages"] if m["kind"] == "error"]
        check("error message at the right place", any("foo_bar" in m["text"] and t2[m["start"]:m["end"]] == "foo_bar" for m in errs), errs)
        check("failed count", node["status"]["failed"] >= 1, node["status"])
    t2 = h.text[main]
    rev = h.edit(main, [(t2.index("exact foo_bar"), t2.index("exact foo_bar") + len("exact foo_bar"), "sorry")])
    node = await h.settled(main, rev)
    check("a sorry proof replaces the error", node is not None and node["status"]["failed"] == 0
          and names(node).count("sorry") == 2, node and node["status"])
    # non-BMP characters and combining marks before the error
    t3 = h.text[main]
    rev = h.edit(main, [(0, 0, "-- 🙂 𝔸 é\n"), (t3.index("def double") + len("-- 🙂 𝔸 é\n"), t3.index("def double") + len("-- 🙂 𝔸 é\n"), "def bad : Nat := \"x\"\n\n")])
    node = await h.settled(main, rev)
    t4 = h.text[main]
    if node:
        errs = [m for m in node["messages"] if m["kind"] == "error"]
        check("offsets are code points after non-BMP text", any(t4[m["start"]:m["end"]] == '"x"' for m in errs), [(m["start"], m["end"], m["text"][:40]) for m in errs])
    else:
        check("offsets are code points after non-BMP text", False)
    rev = h.edit(main, [(t4.index("def bad : Nat := \"x\"\n\n"), t4.index("def bad : Nat := \"x\"\n\n") + len("def bad : Nat := \"x\"\n\n"), "")])
    await h.settled(main, rev)

    # ----- Try this
    t5 = h.text[main]
    at = t5.index("sorry", t5.index("· sorry"))
    rev = h.edit(main, [(at, at + 5, "exact?")])
    node = await h.settled(main, rev)
    if node:
        sb = [s for m in node["messages"] for s in m["sendbacks"]]
        check("exact? gives a message with a sendback to Insert", "exact hq" in sb, [m["text"] for m in node["messages"]])
        tm = [m for m in node["messages"] if m["sendbacks"]]
        check("the Try this message is information on the tool command", bool(tm) and tm[0]["kind"] == "information"
              and h.text[main][tm[0]["start"]:tm[0]["end"]] == "exact?", tm)
    else:
        check("exact? gives a message with a sendback to Insert", False)
    t6 = h.text[main]
    rev = h.edit(main, [(t6.index("exact?"), t6.index("exact?") + 6, "sorry")])
    await h.settled(main, rev)

    # ----- hover and definition
    t7 = h.text[main]
    r = await drv.request({"op": "info", "file": main, "offset": t7.index("Nat", t7.index("def double")) + 1})
    tip = r["info"]["tooltip"]
    check("hover on Nat", bool(tip) and "Nat" in tip["items"][0]["text"] and t7[tip["start"]:tip["end"]] == "Nat", r)
    r = await drv.request({"op": "info", "file": main, "offset": t7.index("add_comm' a b") + 2})
    d = r["info"]["definition"]
    check("definition in the same file", bool(d) and d["file"] == main and d["name"] == "add_comm'" and d["kind"] == "theorem"
          and t7[d["start"]:d["end"]] == "add_comm'" and d["line"] == t7.count("\n", 0, d["start"]) + 1, d)
    r = await drv.request({"op": "info", "file": main, "offset": t7.index("Nat", t7.index("def double")) + 1})
    d = r["info"]["definition"]
    check("definition in the toolchain's sources", bool(d) and d["file"].endswith("Init/Prelude.lean") and d["name"] == "Nat", d)

    # ----- Pause and resume, cancel
    count = len(sess.events)
    drv.send({"op": "checking", "on": False})
    rev = h.edit(main, [(0, 0, "-- paused edit\n")])
    await asyncio.sleep(1.0)
    r = await drv.request({"op": "state", "file": main, "offset": h.text[main].index("theorem and_intro")})
    check("while paused, state still answers from the buffer", r.get("command") is not None, r)
    done, _ = await h.query("deductum_goals", main, 20, timeout=10)
    check("while paused, a query fails", bool(done) and done["status"] == "failed", done)
    drv.send({"op": "checking", "on": True})
    node = await h.settled(main, rev)
    check("Resume rechecks the edited text", node is not None and node["seq"] == rev)
    check("cancel on an idle prover", (await drv.request({"op": "cancel"})) == {"canceled": 0})
    check("cancel_tools", (await drv.request({"op": "cancel_tools"})) == {})
    # a long-running check is interrupted by closing and reopening
    t8 = h.text[main]
    slow = t8.index("decide")
    rev = h.edit(main, [(slow, slow + 6, "simp [Nat.add_comm]\n  all_goals first | decide | omega")])
    r = await drv.request({"op": "cancel"})
    check("cancel returns a count", isinstance(r.get("canceled"), int), r)
    node = await h.settled(main, rev)
    check("after cancel the file is checked again", node is not None)

    # ----- imports changed on disk
    use = os.path.join(root, "Tiny", "UseDep.lean")
    dep = os.path.join(root, "Tiny", "Dep.lean")
    proc = subprocess.run(["lake", "build"], cwd=root, env=tool_env(), capture_output=True, text=True)
    check("lake build of the project", proc.returncode == 0, proc.stdout[-300:] + proc.stderr[-300:])
    h.open(use)
    node = await h.settled(use)
    check("a file with an import is checked", node is not None and node["imports"] == [dep], node and node["imports"])
    with open(dep, "a") as f:
        f.write("def depOther : Nat := 2\n")
    drv.send({"op": "disk_changed", "files": [dep]})
    n = await sess.wait(lambda e: e.get("event") == "node" and e["file"] == use and e["header_errors"], 30)
    check("a changed import shows as a header error", bool(n) and n["header_errors"] == [STALE_HEADER_ERROR], n and n["header_errors"])
    check("the stale-import message is not among the messages", bool(n) and not any("out of date" in m["text"] for m in n["messages"]))
    res = await drv.project_action("build")
    check("project_action build", res["ok"], res)
    logs = [e for e in sess.events if e.get("event") == "log" and "Build completed" in e.get("text", "")]
    check("build output is streamed as log events", bool(logs))
    h.rev[use] += 0
    r = await drv.request({"op": "rebuild_imports", "file": use})
    check("rebuild_imports", r == {}, r)
    start = len(sess.events)
    n = await sess.wait(lambda e: e.get("event") == "node" and e["file"] == use and e["status"]["consolidated"]
                        and not e["header_errors"], 60, start)
    check("after rebuilding, the header error is gone", n is not None)

    # ----- close, stop
    drv.send({"op": "close", "file": use})
    await asyncio.sleep(0.3)
    proc = drv.proc
    await drv.stop()
    check("stop ends the server", proc.returncode is not None)
    shutil.rmtree(root, ignore_errors=True)


# ---------------------------------------------------------------------------
# (b) DSRVLean


def fetch_dsrv(path):
    if os.path.isdir(path):
        return True
    os.makedirs(os.path.dirname(path), exist_ok=True)
    r = subprocess.run(["scp", "-r", "d59500:~/Documents/Research/Repositories/DSRVLean", path],
                       capture_output=True, text=True)
    return r.returncode == 0


async def run_dsrv(path):
    print("== DSRVLean copy at", path, flush=True)
    if not fetch_dsrv(path):
        check("fetch DSRVLean from au-laptop", False, "scp failed")
        return
    check("a scratch copy of DSRVLean", os.path.isfile(os.path.join(path, "lakefile.toml")))
    det = LeanDriver.detect(os.path.join(path, "DSRV"))
    check("detect: toolchain v4.19.0, no Mathlib", bool(det) and det["toolchain"] == "leanprover/lean4:v4.19.0"
          and det["project"]["mathlib"] is False, det)
    t0 = time.monotonic()
    proc = subprocess.run(["lake", "build"], cwd=path, env=tool_env(), capture_output=True, text=True)
    check("lake build of the copy (%.0fs)" % (time.monotonic() - t0), proc.returncode == 0, proc.stdout[-300:] + proc.stderr[-300:])
    h = Harness(path)
    drv, sess = h.drv, h.session
    ready, _ = await h.start()
    check("ready", bool(ready) and ready["event"] == "ready", ready)
    if not ready or ready["event"] != "ready":
        return
    f = os.path.join(path, "DSRV", "CausalCorrectness.lean")
    text = h.open(f)
    t0 = time.monotonic()
    node = await h.settled(f, timeout=600)
    check("CausalCorrectness is checked (%.0fs)" % (time.monotonic() - t0), node is not None)
    if node is None:
        return
    check("many commands, none overlapping", len(node["commands"]) > 200 and all(
        a[1] <= b[0] for a, b in zip(node["commands"], node["commands"][1:])), len(node["commands"]))
    check("no failures in the pristine file", node["status"]["failed"] == 0, node["status"])
    print("     commands: %d, messages: %d, colours: %d" % (len(node["commands"]), len(node["messages"]), len(node["colours"])))
    # the induction in the first theorem
    zero = text.index("| zero => simp [evalOne, InSupport] at reported")
    simp_at = text.index("simp [evalOne, InSupport] at reported", zero)
    st = status_of(node, text, "simp [evalOne, InSupport] at reported", "simp")
    check("a tactic inside a proof is a finished command", st == "finished", st)
    for label, needle, delta in (("start of the theorem proof", "induction fuel generalizing", 0),
                                 ("inside the zero case", "simp [evalOne, InSupport] at reported", 0),
                                 ("the succ case", "cases expression with", 0),
                                 ("a bullet", "· exact Or.inl (ih static current left", 2)):
        off = text.index(needle) + delta
        done, _ = await h.query("deductum_goals", f, off, timeout=300)
        ok = bool(done) and done["status"] == "finished"
        parts = goal_kinds(done["messages"]) if ok else {}
        check("goals: " + label, ok and "concl" in parts and parts["count"][0][0] >= 1, done)
        if ok and label == "start of the theorem proof":
            print("     ", parts["concl"][0][1][:100].replace("\n", " "))
            check("goals: hypotheses of the theorem", any("fuel" in b for _, b in parts["param"]), parts["param"][:3])
    # an edit: put a sorry where the first proof step is, then check a candidate in its place
    rev = h.edit(f, [(simp_at, simp_at + len("simp [evalOne, InSupport] at reported"), "sorry")])
    node = await h.settled(f, rev, timeout=600)
    check("sorry edit: the theorem is warned, nothing failed", node is not None and node["status"]["failed"] == 0
          and status_of(node, h.text[f], "theorem evalOne_support_subset_operationalFootprint") == "warned", node and node["status"])
    s_at = simp_at
    done, _ = await h.query("deductum_goals", f, s_at, timeout=300)
    check("goals at the sorry", bool(done) and done["status"] == "finished" and done.get("replace") == {"start": s_at, "end": s_at + 5}, done)
    good = "simp [evalOne, InSupport] at reported"
    t0 = time.monotonic()
    done, _ = await h.query("deductum_command", f, s_at, [good], timeout=300)
    check("candidate accepted (%.1fs)" % (time.monotonic() - t0), bool(done) and done["status"] == "finished"
          and not [m for m in done["messages"] if m["kind"] == "error"], done)
    done, _ = await h.query("deductum_command", f, s_at, ["exact rfl"], timeout=300)
    check("wrong candidate rejected", bool(done) and any(m["kind"] == "error" for m in done["messages"]), done)
    # a type error
    t = h.text[f]
    at = t.index("(core : IsCore expression)")
    rev = h.edit(f, [(at, at + len("(core : IsCore expression)"), "(core : IsCore 5)")])
    node = await h.settled(f, rev, timeout=600)
    check("a type error is reported with the revision", node is not None and node["status"]["failed"] >= 1
          and any(m["kind"] == "error" for m in node["messages"]), node and node["status"])
    # put the file back
    rev = h.edit(f, [(0, len(h.text[f]), text)])
    node = await h.settled(f, rev, timeout=600)
    check("restored text checks cleanly again", node is not None and node["status"]["failed"] == 0, node and node["status"])
    # hover and definition
    off = text.index("evalOne", text.index("reported : InSupport")) + 2
    r = await drv.request({"op": "info", "file": f, "offset": off})
    check("hover on evalOne", bool(r["info"]["tooltip"]), r)
    d = r["info"]["definition"]
    check("definition of evalOne in another file", bool(d) and d["file"].endswith("CausalPaperSemantics.lean")
          and d["name"] == "evalOne", d)
    # pause / resume
    drv.send({"op": "checking", "on": False})
    await asyncio.sleep(0.5)
    drv.send({"op": "checking", "on": True})
    node = await h.settled(f, timeout=600)
    check("pause and resume", node is not None)
    await drv.stop()
    check("stop", drv.proc.returncode is not None)


# ---------------------------------------------------------------------------
# (c) the real host core with a Lean session


def run_core(path):
    """read, edit, state, goals, a keyword fill checked by deductum_command, pause/resume, stop"""
    print("== host core with a Lean session on a copy of", path, flush=True)
    state = tempfile.mkdtemp(prefix="deductum-lean-state-", dir=SCRATCH)
    os.environ["DEDUCTUM_STATE_DIR"] = state
    fake = tempfile.mkdtemp(prefix="deductum-bin-", dir=SCRATCH)
    with open(os.path.join(fake, "claude"), "w") as f:
        f.write("#!/bin/sh\necho '{\"proofs\": [\"simp [evalOne, InSupport] at reported\", \"exact rfl\"], "
                "\"using\": [\"Nat.add_comm\", \"no_such_fact_xyz\"]}'\n")
    os.chmod(os.path.join(fake, "claude"), 0o755)
    os.environ["PATH"] = fake + os.pathsep + os.environ.get("PATH", "")
    work = path.rstrip("/") + "-core"
    shutil.rmtree(work, ignore_errors=True)
    shutil.copytree(path, work)
    sys.path.insert(0, os.path.join(HERE, ".."))
    import deductum_host as H
    f = os.path.join(work, "DSRV", "CausalCorrectness.lean")
    text = open(f, encoding="utf-8").read()

    def wait_for(client, pred, timeout=300, what="event"):
        t0 = time.time()
        while time.time() - t0 < timeout:
            for i, ev in enumerate(client.events):
                if pred(ev):
                    del client.events[:i + 1]
                    return ev
            client.events.clear()
            try:
                client.sock.settimeout(1.0)
                client.events.append(client.read_message())
            except (TimeoutError, OSError):
                pass
            finally:
                client.sock.settimeout(None)
        return None

    app = H.SyncClient.local()
    try:
        app.request("hello", client="app", label="lean test", concurrent_reads=True)
        detected = app.request("detect", folder=work) if False else None
        s = app.request("session_create", folder=work, backend="lean", new=True)["session"]
        sid = s["id"]
        check("core: session starts", s["lifecycle"] == "Starting" and s["backend"] == "lean", {k: s.get(k) for k in ("lifecycle", "backend")})
        doc = app.request("read", session=sid, path=f)["doc"]
        ev = wait_for(app, lambda e: e.get("event") == "session" and e["session"]["id"] == sid
                      and e["session"]["lifecycle"] in ("Running", "Failed"))
        check("core: Running", bool(ev) and ev["session"]["lifecycle"] == "Running", ev and ev["session"].get("message"))
        ev = wait_for(app, lambda e: e.get("event") == "doc_prover" and e["prover"]["status"]
                      and e["prover"]["status"].get("consolidated"))
        check("core: the file is checked", bool(ev))
        names_ = [c[2] for c in ev["prover"]["commands"]] if ev else []
        check("core: command spans reach the core", "theorem" in names_ and "induction" in names_, names_[:5])
        info = app.request("session_info", session=sid)["session"]
        caps = info.get("capabilities") or {}
        check("core: capabilities reach the app", caps.get("goal_style") == "lean" and caps.get("placeholder") == "sorry", caps)
        # an edit: the zero case becomes a keyword line
        old = "simp [evalOne, InSupport] at reported"
        at = text.index(old, text.index("| zero =>"))
        app.request("edit", session=sid, path=f, revision=doc["revision"],
                    edits=[{"start": at, "end": at + len(old), "text": "claude try simp"}], txn="kw")
        doc = app.request("read", session=sid, path=f)["doc"]
        r = app.request("keyword", session=sid, path=f, offset=at + 3, revision=doc["revision"])
        ev = wait_for(app, lambda e: e.get("event") == "fill" and e.get("replace"), timeout=60)
        check("core: a claude line becomes a hole", bool(ev) and r.get("started"), (ev, r))
        ev = wait_for(app, lambda e: e.get("event") == "fill" and e.get("status") in ("done", "failed"), timeout=300)
        ok = [c.get("ok") for c in (ev or {}).get("candidates", [])]
        check("core: candidates are checked by the Lean scratch document", ok == [True, False], ev)
        check("core: suggested facts are kept only if they exist", (ev or {}).get("using") == ["Nat.add_comm"], ev and ev.get("using"))
        # state and goals at the hole
        doc = app.request("read", session=sid, path=f)["doc"]
        hole = doc["text"].index("claude try simp")
        st = None
        for _ in range(120):  # (the hole is a command once the prover has reported this revision)
            try:
                st = app.request("state", session=sid, path=f, offset=hole)
                if any(m["kind"] == "state" and "⊢" in m["text"] for m in st["messages"]) and not st["outdated"]:
                    break
            except H.HostError:
                pass
            time.sleep(1)
        check("core: state at the hole", st is not None and any(m["kind"] == "state" and "⊢" in m["text"] for m in st["messages"]), st)
        q = app.request("query", session=sid, path=f, offset=hole, operation="deductum_goals", args=["100", "false"])["query"]
        ev = wait_for(app, lambda e: e.get("event") == "query" and e.get("id", e.get("query")) == q
                      and e["status"] in ("finished", "failed"), timeout=120)
        check("core: deductum_goals reaches the app", bool(ev) and ev["status"] == "finished" and ev.get("replace")
              and any(m["raw"].startswith("#deductum:concl") for m in ev["messages"]), ev)
        # pause and resume
        app.request("pause", session=sid)
        try:
            app.request("state", session=sid, path=f, offset=hole)
            check("core: paused session refuses queries", False)
        except H.HostError as e:
            check("core: paused session refuses queries", e.code == "paused", e.code)
        doc = app.request("read", session=sid, path=f)["doc"]
        app.request("edit", session=sid, path=f, revision=doc["revision"],
                    edits=[{"start": 0, "end": 0, "text": "-- while paused\n"}], txn="p")
        app.request("resume", session=sid)
        ev = wait_for(app, lambda e: e.get("event") == "doc_prover" and e["prover"]["status"]
                      and e["prover"]["status"].get("consolidated") and e["prover"].get("backend_revision", 0) >= doc["revision"] + 1,
                      timeout=300)
        check("core: resume rechecks the buffer", bool(ev))
        app.request("stop", session=sid)
        info = app.request("session_info", session=sid)["session"]
        check("core: stop", info["lifecycle"] == "Stopped", info["lifecycle"])
    finally:
        with contextlib.suppress(Exception):
            app.request("shutdown")
        shutil.rmtree(work, ignore_errors=True)


async def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dsrv", default=os.path.join(SCRATCH, "DSRVLean"))
    ap.add_argument("--skip-dsrv", action="store_true")
    ap.add_argument("--core-fixture", help="self-contained Lake project for the existing host-core assertions")
    ap.add_argument("--skip-tiny", action="store_true")
    ap.add_argument("--skip-core", action="store_true")
    ap.add_argument("--only-core", action="store_true", help="only the host-core session on the DSRVLean copy")
    args = ap.parse_args()
    os.makedirs(SCRATCH, exist_ok=True)
    if not args.skip_tiny and not args.only_core:
        await run_tiny()
    if not args.skip_dsrv and not args.only_core:
        await run_dsrv(args.dsrv)
    if not args.skip_core and args.core_fixture:
        run_core(args.core_fixture)
    elif not args.skip_core and not args.skip_dsrv:
        run_core(args.dsrv)
    print("\n%s" % ("ALL PASSED" if FAILURES == 0 else "%d FAILED" % FAILURES))
    return FAILURES


if __name__ == "__main__":
    sys.exit(min(asyncio.run(main()), 100))
