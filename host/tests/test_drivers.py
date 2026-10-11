"""The host core without a prover: session records across backends, detection, the registry,
document filtering, open policy, the memory budget and the Isabellum migration. A FakeDriver
stands in for a prover; nothing here needs Isabelle or Lean."""
import asyncio
import json
import os
import socket
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
os.environ.setdefault("DEDUCTUM_STATE_DIR", tempfile.mkdtemp())
import deductum_host as H  # noqa: E402
import drivers  # noqa: E402
from drivers.isabelle import IsabelleDriver  # noqa: E402


class FakeDriver(drivers.Driver):
    backend = "fake"
    label = "Fake"
    documents = (".fk",)
    project_files = ("FAKEROOT",)
    open_policy = "viewed"
    sent: list = []

    @classmethod
    def toolchains(cls):
        return [{"id": "fake-1", "label": "Fake 1", "version": "1", "path": "/fake", "supported": True}]

    @classmethod
    def detect(cls, folder):
        if (Path(folder) / "FAKEROOT").is_file():
            return {"backend": "fake", "evidence": ["FAKEROOT"], "project": {"root": folder},
                    "toolchain": "fake-1", "note": None}
        return None

    def capabilities(self):
        return {"documents": [".fk"], "placeholder": "sorry", "checking": "continuous"}

    async def start(self):
        # (a real process, so that the memory budget has something to measure)
        self.proc = await asyncio.create_subprocess_exec("sleep", "60", start_new_session=True)
        self.emit({"event": "ready", "identifier": "Fake 1"})

    def send(self, obj):
        FakeDriver.sent.append(obj)

    async def request(self, obj, timeout=120.0):
        return {"syntax": {"kinds": {}}}

    async def stop(self):
        if self.proc and self.proc.returncode is None:
            self.proc.terminate()
            await self.proc.wait()


class App:
    """A stand-in client"""
    kind = "app"
    key = None
    id = "app1"
    label = "test"
    location: dict = {}

    def __init__(self):
        self.sessions = set()
        self.open_docs = set()
        self.views = {}
        self.pushed = []
        self.closed = False

    def author(self):
        return H.HUMAN

    def push(self, obj):
        self.pushed.append(obj)

    def attach(self, session):
        self.sessions.add(session.id)

    def note_position(self, *a):
        pass


class Agent(App):
    kind = "agent"
    key = "claude:conv"
    id = "agent1"

    def author(self):
        return {"kind": "agent", "key": self.key, "label": "Claude"}


OLD_META = {"id": "old1", "name": "T", "folder": "/tmp/x", "runtime": "/opt/Isabelle2025/bin/isabelle",
            "identifier": "Isabelle2025", "logic": "HOL-Library", "dirs": ["/tmp/x"],
            "include_sessions": ["S"], "lifecycle": "Stopped"}


class DriverTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self._state = H.STATE_DIR
        H.STATE_DIR = self.tmp / "state"
        self._xdg = os.environ.get("XDG_CONFIG_HOME")
        os.environ["XDG_CONFIG_HOME"] = str(self.tmp / "config")
        drivers.register(FakeDriver)
        FakeDriver.sent = []
        self.host = H.Host()

    async def asyncTearDown(self):
        for s in list(self.host.sessions.values()):
            await s.stop()

    def tearDown(self):
        drivers.unregister("fake")
        H.STATE_DIR = self._state
        if self._xdg is None:
            os.environ.pop("XDG_CONFIG_HOME", None)
        else:
            os.environ["XDG_CONFIG_HOME"] = self._xdg

    def folder(self, *files):
        d = Path(tempfile.mkdtemp(dir=self.tmp))
        for f in files:
            (d / f).parent.mkdir(parents=True, exist_ok=True)
            (d / f).write_text("x")
        return d

    # ----- records

    def test_an_old_isabelle_record_loads_as_isabelle(self):
        s = H.Session(self.host, "old1", json.loads(json.dumps(OLD_META)))
        m = s.meta
        self.assertEqual(m["backend"], "isabelle")
        self.assertEqual(m["toolchain"], OLD_META["runtime"])
        self.assertEqual(m["project"], {"logic": "HOL-Library", "dirs": ["/tmp/x"], "include_sessions": ["S"]})
        # the app still reads the old top-level keys
        for k in ("logic", "runtime", "dirs", "include_sessions"):
            self.assertEqual(m[k], OLD_META[k])
        summary = s.summary()
        self.assertEqual(summary["backend"], "isabelle")
        self.assertEqual(summary["toolchain"], {"id": OLD_META["runtime"], "label": "Isabelle2025"})
        self.assertEqual(summary["logic"], "HOL-Library")
        self.assertEqual(summary["documents"], [".thy"])
        self.assertEqual(summary["project"]["logic"], "HOL-Library")
        caps = summary["capabilities"]
        self.assertEqual(caps["placeholder"], "sorry")
        self.assertEqual(caps["facts_line"], "using {facts}\nsledgehammer")
        self.assertEqual(caps["pause"], "perspective")
        self.assertEqual([t["name"] for t in caps["tools"]][:2], ["sledgehammer", "try"])
        self.assertEqual(caps["counterexamples"], ["quickcheck", "nitpick"])

    def test_configure_keeps_the_project_in_step_with_the_old_keys(self):
        s = H.Session(self.host, "old1", json.loads(json.dumps(OLD_META)))
        IsabelleDriver.sync_meta(s.meta, {"logic": "HOL"})
        self.assertEqual(s.meta["project"]["logic"], "HOL")
        IsabelleDriver.sync_meta(s.meta, {"project": {"include_sessions": []}})
        self.assertEqual(s.meta["include_sessions"], [])

    # ----- discovery

    def test_detection(self):
        thy = self.folder("ROOT", "A.thy")
        d = IsabelleDriver.detect(str(thy))
        self.assertEqual(d["backend"], "isabelle")
        self.assertTrue(any("ROOT" in e for e in d["evidence"]))
        self.assertEqual(set(d["project"]), {"logic", "dirs", "include_sessions", "session", "sessions"})
        self.assertIsNone(IsabelleDriver.detect(str(self.folder("README"))))
        lake = self.folder("lakefile.toml", "Main.lean")
        lean = drivers.registry().get("lean")
        if lean is None:
            self.skipTest("the Lean driver is not importable")
        self.assertEqual(lean.detect(str(lake))["backend"], "lean")
        self.assertIsNone(IsabelleDriver.detect(str(lake)))

    def test_the_registry(self):
        reg = drivers.registry()
        self.assertIs(reg["isabelle"], IsabelleDriver)
        self.assertIs(reg["fake"], FakeDriver)
        drivers.unregister("fake")
        self.assertNotIn("fake", drivers.registry())

    def test_the_backend_is_picked_by_detection(self):
        self.assertEqual(self.host.detect_backend(str(self.folder("FAKEROOT"))), "fake")
        self.assertEqual(self.host.detect_backend(str(self.folder("ROOT"))), "isabelle")
        # nothing detects it (or several do): Isabelle, as before there were backends
        self.assertEqual(self.host.detect_backend(str(self.folder("README"))), "isabelle")

    async def test_hello_suggest_and_toolchains(self):
        app = App()
        r = await self.host.handle(app, {"op": "hello", "client": "app"})
        self.assertEqual(r["protocol"], 2)
        self.assertIn("runtimes", r)
        backends = {b["backend"]: b for b in r["backends"]}
        self.assertEqual(backends["isabelle"]["documents"], [".thy"])
        self.assertEqual(backends["isabelle"]["project_files"], ["ROOT", "ROOTS"])
        self.assertEqual(backends["fake"]["toolchains"][0]["id"], "fake-1")
        folder = self.folder("FAKEROOT")
        r = await self.host.handle(app, {"op": "suggest", "folder": str(folder)})
        self.assertEqual([d["backend"] for d in r["detections"]], ["fake"])
        self.assertIn("suggestion", r)
        self.assertIn("runtimes", r)
        r = await self.host.handle(app, {"op": "toolchains", "backend": "fake"})
        self.assertEqual([t["backend"] for t in r["toolchains"]], ["fake"])
        with self.assertRaises(H.HostError):
            await self.host.handle(app, {"op": "toolchains", "backend": "nope"})
        self.assertEqual((await self.host.handle(app, {"op": "symbol_table", "backend": "fake"}))["symbols"], [])

    async def test_list_dir_filters_by_the_backends_documents(self):
        d = self.folder("A.thy", "ROOT", "B.fk", "FAKEROOT", "C.txt", "sub/x")
        app = App()
        names = lambda r: sorted(e["name"] for e in r["entries"] if not e["dir"])
        r = await self.host.handle(app, {"op": "list_dir", "path": str(d)})
        self.assertEqual(names(r), ["A.thy", "B.fk", "FAKEROOT", "ROOT"])
        s = H.Session(self.host, "f1", {"folder": str(d), "backend": "fake", "toolchain": "fake-1",
                                        "project": {}, "lifecycle": "Stopped"})
        self.host.sessions["f1"] = s
        r = await self.host.handle(app, {"op": "list_dir", "path": str(d), "session": "f1"})
        self.assertEqual(names(r), ["B.fk", "FAKEROOT"])
        self.assertEqual(sorted(os.path.basename(p) for p in H.list_documents(str(d), classes=[IsabelleDriver])),
                         ["A.thy", "ROOT"])

    # ----- sessions

    def test_find_session_is_keyed_by_backend_toolchain_and_project(self):
        d = str(self.folder("x"))
        a = H.Session(self.host, "a", {"folder": d, "backend": "fake", "toolchain": "fake-1",
                                       "project": {"p": 1}, "lifecycle": "Stopped"})
        b = H.Session(self.host, "b", {"folder": d, "runtime": "/rt/bin/isabelle", "logic": "HOL",
                                       "lifecycle": "Stopped"})
        self.host.sessions.update(a=a, b=b)
        self.assertIs(self.host.find_session(d, "fake", "fake-1", {"p": 1}), a)
        self.assertIsNone(self.host.find_session(d, "fake", "fake-2", {"p": 1}))
        self.assertIsNone(self.host.find_session(d, "fake", "fake-1", {"p": 2}))
        # for Isabelle: (runtime, logic)
        self.assertIs(self.host.find_session(d, "isabelle", "/rt/bin/isabelle", {"logic": "HOL", "dirs": ["z"]}), b)
        self.assertIsNone(self.host.find_session(d, "isabelle", "/rt/bin/isabelle", {"logic": "HOL-Library"}))

    async def test_a_session_of_a_driver_runs_and_reports_capabilities(self):
        d = self.folder("FAKEROOT", "A.fk")
        s = self.host.create_session({"folder": str(d)})  # detected: fake
        self.assertEqual(s.meta["backend"], "fake")
        self.assertEqual(s.meta["toolchain"], "fake-1")
        for _ in range(50):
            if s.lifecycle == "Running":
                break
            await asyncio.sleep(0.05)
        self.assertEqual(s.lifecycle, "Running")
        self.assertEqual(s.summary()["capabilities"]["checking"], "continuous")
        self.assertIs(self.host.create_session({"folder": str(d), "backend": "fake"}), s)

    async def test_viewed_documents_are_open_in_the_prover_only_while_wanted(self):
        d = self.folder("FAKEROOT", "A.fk")
        s = self.host.create_session({"folder": str(d), "backend": "fake"})
        for _ in range(50):
            if s.lifecycle == "Running":
                break
            await asyncio.sleep(0.05)
        app = App()
        self.host.clients.append(app)
        path = str(d / "A.fk")
        doc = s.get_doc(path)
        app.open_docs.add((s.id, doc.path))
        self.assertEqual([o["op"] for o in FakeDriver.sent], ["open"])
        s.close_unviewed()
        self.assertEqual([o["op"] for o in FakeDriver.sent], ["open"])
        # not viewed any more, and no agent asked lately: closed, but the buffer stays
        app.open_docs.clear()
        s.close_unviewed()
        self.assertEqual(s.driver_class.open_policy, "viewed")
        self.assertEqual([o["op"] for o in FakeDriver.sent], ["open", "close"])
        self.assertIn(doc.path, s.docs)
        # an agent asks: it is opened again, and stays for ten minutes
        agent = Agent()
        await self.host.handle(agent, {"op": "read", "session": s.id, "path": path, "subscribe": False})
        self.assertEqual([o["op"] for o in FakeDriver.sent], ["open", "close", "open"])
        s.close_unviewed()
        self.assertEqual(len(FakeDriver.sent), 3)
        doc.agent_query -= 700
        s.close_unviewed()
        self.assertEqual([o["op"] for o in FakeDriver.sent][-1], "close")

    async def test_isabelle_keeps_every_document_open(self):
        s = H.Session(self.host, "i1", json.loads(json.dumps(OLD_META)))
        doc = H.Doc(s, "/tmp/x/A.thy")
        self.assertTrue(s.wanted_in_prover(doc))

    async def test_the_memory_budget(self):
        d = self.folder("FAKEROOT")
        s = self.host.create_session({"folder": str(d), "backend": "fake"})
        for _ in range(50):
            if s.lifecycle == "Running":
                break
            await asyncio.sleep(0.05)
        self.host.check_memory_budget()  # no budget configured: no limit
        cfg = drivers.config_path()
        cfg.parent.mkdir(parents=True)
        cfg.write_text(json.dumps({"memory_budget_gb": 1000}))
        self.host.check_memory_budget()
        cfg.write_text(json.dumps({"memory_budget_gb": 0.0001}))
        with self.assertRaises(H.HostError) as cm:
            self.host.check_memory_budget()
        self.assertEqual(cm.exception.code, "memory_budget")
        self.assertIn("memory budget", str(cm.exception))
        with self.assertRaises(H.HostError):
            self.host.create_session({"folder": str(self.folder("FAKEROOT")), "backend": "fake", "new": True})
        await s.stop()
        self.host.check_memory_budget()

    async def test_project_files_stay_out_of_the_prover(self):
        d = self.folder("FAKEROOT", "A.fk")
        s = self.host.create_session({"folder": str(d), "backend": "fake"})
        for _ in range(50):
            if s.lifecycle == "Running":
                break
            await asyncio.sleep(0.05)
        app = App()
        self.host.clients.append(app)
        root = s.get_doc(str(d / "FAKEROOT"))
        app.open_docs.add((s.id, root.path))
        self.assertEqual(FakeDriver.sent, [], "a project file is the host's buffer only")
        s.get_doc(str(d / "A.fk"))
        self.assertEqual([o["op"] for o in FakeDriver.sent], ["open"])

    async def test_the_apps_settings_decide_its_keyword_lines(self):
        d = self.folder("FAKEROOT", "A.fk")
        (d / "A.fk").write_text("lemma x\n  claude\n  codex[astra]\n")
        s = self.host.create_session({"folder": str(d), "backend": "fake"})
        for _ in range(50):
            if s.lifecycle == "Running":
                break
            await asyncio.sleep(0.05)
        doc = s.get_doc(str(d / "A.fk"))
        asked = []
        s.fill_hole = lambda client, path, start, rev, agent, hint, **kw: asked.append((agent, kw.get("model"))) or {"job": str(len(asked))}
        app = App()
        app.settings = {}
        await self.host.handle(app, {"op": "client_settings", "keywords": False, "models": {"claude": "opus"}, "other": 1})
        self.assertEqual(app.settings, {"keywords": False, "models": {"claude": "opus"}})
        line = doc.text.index("  claude")
        self.assertFalse(s.keyword_line(app, doc, line, app.author(), "t1"), "keyword lines are off")
        app.settings["keywords"] = True
        self.assertTrue(s.keyword_line(app, doc, line, app.author(), "t2"))
        self.assertTrue(s.keyword_line(app, doc, doc.text.index("  codex"), app.author(), "t3"))
        self.assertEqual(asked, [("claude", "opus"), ("codex", "astra")], "the line's own model wins")

    async def test_project_action_is_unknown_for_isabelle(self):
        s = H.Session(self.host, "i1", json.loads(json.dumps(OLD_META)))
        self.host.sessions["i1"] = s
        with self.assertRaises(H.HostError) as cm:
            await self.host.handle(App(), {"op": "project_action", "session": "i1", "name": "build"})
        self.assertIn("unknown action", str(cm.exception))

    # ----- independent sessions

    async def test_forks_work_for_any_backend_and_link_lake_packages(self):
        repo = self.folder("A.fk", "FAKEROOT", ".lake/packages/dep/x.txt")
        env = {"GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t", "GIT_COMMITTER_NAME": "t",
               "GIT_COMMITTER_EMAIL": "t@t", **os.environ}
        for args in (["init", "-q"], ["add", "A.fk", "FAKEROOT"], ["commit", "-qm", "init"]):
            subprocess.run(["git", *args], cwd=repo, check=True, env=env, capture_output=True)
        agent = Agent()
        s = self.host.fork_session(agent, {"folder": str(repo), "backend": "fake", "start": False})
        parent = s.meta["parent"]
        self.assertEqual(s.meta["backend"], "fake")
        self.assertRegex(parent["branch"], r"^deductum/[0-9a-f]{8}$")
        wt = Path(parent["worktree"])
        self.assertTrue((wt / "A.fk").is_file())
        self.assertTrue((wt / ".lake" / "packages").is_symlink())
        self.assertEqual(os.path.realpath(wt / ".lake" / "packages"), os.path.realpath(repo / ".lake" / "packages"))
        doc = s.get_doc(str(wt / "A.fk"))
        s.commit(doc, agent.author(), "edit", [(0, 1, "changed")])
        files = self.host.merge_preview(s)
        self.assertEqual([f["rel"] for f in files], ["A.fk"])
        self.assertEqual(s.meta["owner"]["key"], "claude:conv")


class MigrationTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.old_state = self.tmp / "old" / "isabellum"
        self.new_state = self.tmp / "new" / "deductum"
        self.old_cfg = self.tmp / "oldcfg" / "host.json"
        self.new_cfg = self.tmp / "newcfg" / "host.json"
        (self.old_state / "sessions" / "abc12345" / "docs").mkdir(parents=True)
        (self.old_state / "sessions" / "abc12345" / "meta.json").write_text(json.dumps({"id": "abc12345"}))
        self.old_cfg.parent.mkdir(parents=True)
        self.old_cfg.write_text(json.dumps({"runtimes": ["~/Isabelle/bin/isabelle"], "other": 1}))
        self.said = []

    def run_migration(self):
        return H.migrate_from_isabellum(self.old_state, self.old_cfg, self.new_state, self.new_cfg,
                                        say=self.said.append)

    def test_moves_sessions_and_rewrites_the_config(self):
        self.assertEqual(self.run_migration(), 0)
        self.assertTrue((self.new_state / "sessions" / "abc12345" / "meta.json").is_file())
        self.assertFalse((self.old_state / "sessions" / "abc12345").exists())
        cfg = json.loads(self.new_cfg.read_text())
        self.assertEqual(cfg, {"other": 1, "isabelle": [{"path": "~/Isabelle/bin/isabelle"}]})

    def test_independent_sessions_follow_their_worktrees(self):
        wt_old = self.old_state / "worktrees" / "repo-d1"
        wt_old.mkdir(parents=True)
        (wt_old / "A.thy").write_text("x")
        path = str(wt_old / "A.thy")
        d = self.old_state / "sessions" / "d1"
        (d / "docs").mkdir(parents=True)
        (d / "logs").mkdir()
        (d / "meta.json").write_text(json.dumps({"id": "d1", "folder": str(wt_old),
                                                 "parent": {"worktree": str(wt_old), "repo": "/nonexistent"}}))
        (d / "docs" / (H.file_key(path) + ".json")).write_text(json.dumps({"path": path, "text": "x"}))
        (d / "logs" / (H.file_key(path) + ".jsonl")).write_text("{}\n")
        self.assertEqual(self.run_migration(), 0)
        new_wt = self.new_state / "worktrees" / "repo-d1"
        self.assertTrue((new_wt / "A.thy").is_file())
        meta = json.loads((self.new_state / "sessions" / "d1" / "meta.json").read_text())
        self.assertEqual(meta["folder"], str(new_wt))
        new_path = str(new_wt / "A.thy")
        doc = self.new_state / "sessions" / "d1" / "docs" / (H.file_key(new_path) + ".json")
        self.assertEqual(json.loads(doc.read_text())["path"], new_path)
        self.assertTrue((self.new_state / "sessions" / "d1" / "logs" / (H.file_key(new_path) + ".jsonl")).is_file())
        self.assertEqual(len(list((self.new_state / "sessions" / "d1" / "docs").glob("*.json"))), 1)

    def test_does_nothing_while_the_isabellum_host_runs(self):
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.bind(str(self.old_state / "host.sock"))
        sock.listen(1)
        try:
            self.assertEqual(self.run_migration(), 1)
        finally:
            sock.close()
        self.assertTrue((self.old_state / "sessions" / "abc12345").exists())
        self.assertFalse(self.new_state.exists())
        self.assertIn("running", self.said[0])

    def test_does_nothing_when_deductum_has_sessions(self):
        (self.new_state / "sessions" / "zzz").mkdir(parents=True)
        (self.new_state / "sessions" / "zzz" / "meta.json").write_text("{}")
        self.assertEqual(self.run_migration(), 1)
        self.assertTrue((self.old_state / "sessions" / "abc12345").exists())
        self.assertFalse(self.new_cfg.exists())

    def test_a_stale_socket_file_does_not_count_as_running(self):
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.bind(str(self.old_state / "host.sock"))
        sock.close()  # (bound, never listening: connecting fails)
        self.assertEqual(self.run_migration(), 0)

    def test_the_command_with_a_temporary_home(self):
        home = self.tmp / "home"
        state, cfg = home / "state", home / "config"
        (state / "isabellum" / "sessions" / "s1").mkdir(parents=True)
        (state / "isabellum" / "sessions" / "s1" / "meta.json").write_text("{}")
        (cfg / "isabellum").mkdir(parents=True)
        (cfg / "isabellum" / "host.json").write_text(json.dumps({"runtimes": ["/r/bin/isabelle"]}))
        env = {k: v for k, v in os.environ.items() if k != "DEDUCTUM_STATE_DIR"}
        env.update(HOME=str(home), XDG_STATE_HOME=str(state), XDG_CONFIG_HOME=str(cfg))
        r = subprocess.run([sys.executable, H.__file__, "migrate-from-isabellum"], env=env,
                           capture_output=True, text=True, timeout=60)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertTrue((state / "deductum" / "sessions" / "s1" / "meta.json").is_file())
        self.assertEqual(json.loads((cfg / "deductum" / "host.json").read_text()),
                         {"isabelle": [{"path": "/r/bin/isabelle"}]})
        again = subprocess.run([sys.executable, H.__file__, "migrate-from-isabellum"], env=env,
                               capture_output=True, text=True, timeout=60)
        self.assertEqual(again.returncode, 1)  # Deductum has sessions now


class PerBackendText(unittest.TestCase):
    def test_placeholders_and_headers(self):
        text = "lemma x\n  claude\n"
        self.assertEqual(H.prover_text(text, "sorry"), "lemma x\n  sorry \n")
        self.assertEqual(H.prover_text("theorem t : P := by\n  codex\n", "sorry"),
                         "theorem t : P := by\n  sorry\n")
        self.assertEqual(IsabelleDriver.header_of("theory T imports Main begin\nlemma"), "theory T imports Main begin")
        self.assertEqual(IsabelleDriver.fact_check_text(["a", "b"]), "thm a b")
        self.assertEqual(drivers.Driver.fact_check_text(["a"]), None)

    def test_prompts_exist_for_every_backend_and_format(self):
        import prompts
        fields = dict(identifier="v", logic="HOL", marker="<<<GAP>>>", n=3, hint="", header="h", before="b",
                      lemma="l", state="s")
        for backend in ("isabelle", "lean"):
            self.assertIn("<<<GAP>>>", prompts.fill_prompt(backend).format(**fields))
            self.assertIn("using", prompts.facts_prompt(backend).format(**fields))
        self.assertIn("Lean 4", prompts.fill_prompt("lean"))
        self.assertIn("Isabelle", prompts.fill_prompt("unknown"))


if __name__ == "__main__":
    unittest.main()
