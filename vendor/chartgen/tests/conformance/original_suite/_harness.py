"""Run one original unittest module and record every test's outcome (child of run_suite.py).

    python _harness.py STAGE_CHARTS MODULE OUT.json --impl {oracle,rewrite}

STAGE_CHARTS is the staged copy of the original ``charts/`` directory that
run_suite.py prepares (``tests/`` copied from the original, ``examples/specs``
linked to the bundled examples). The original test module is imported from
there as ``tests.<module>``, exactly as ``python -m unittest`` in ``charts/``
would.

Under ``--impl rewrite`` the ``chartgen`` package is the rewrite's (the
installed binding); the stage holds no original ``chartgen``. Every name a
test imports from ``chartgen`` that the rewrite does not provide is replaced by
an *absent-internal placeholder*. Any use of a placeholder (call, attribute,
iteration, comparison, truth value, ...) raises ``OriginalInternalUnavailable``
and is recorded for the running test, even if the test swallows the
exception. A test that touched a placeholder is reported as such; the
classifier treats it as inapplicable (it exercises an original internal), never
as a pass. Nothing is skipped: every test the module defines is run and
recorded.
"""
import ast
import importlib
import json
import sys
import time
import traceback
import types
import unittest
from pathlib import Path

TOUCHED = []          # [(name, test-file frame description)] since the last reset
NATIVE_CALLS = [0]    # rewrite: calls into the Rust engine (chartgen._native.dispatch) since the last reset
INJECTED = {}         # "module.name" -> "module-missing" | "name-missing"


class OriginalInternalUnavailable(Exception):
    """A test used an original chartgen internal that the rewrite's package lacks."""


def _origin():
    for frame in reversed(traceback.extract_stack()[:-2]):
        if "/tests/test_" in frame.filename:
            return f"{Path(frame.filename).name}:{frame.lineno}:{frame.name}"
    return None


class Absent:
    """Stands for an original chartgen internal absent from the rewrite."""

    def __init__(self, name):
        object.__setattr__(self, "_name", name)

    def _touch(self, how):
        name = object.__getattribute__(self, "_name")
        TOUCHED.append((name, how, _origin()))
        raise OriginalInternalUnavailable(f"{name} ({how}) is an original chartgen internal absent from the rewrite")

    def __getattr__(self, attr):
        if attr.startswith("__") and attr.endswith("__"):
            raise AttributeError(attr)
        return self._touch(f"attribute {attr}")

    def __call__(self, *a, **k):
        return self._touch("call")

    def __iter__(self):
        return self._touch("iteration")

    def __getitem__(self, key):
        return self._touch("subscript")

    def __contains__(self, key):
        return self._touch("membership")

    def __len__(self):
        return self._touch("len")

    def __bool__(self):
        return self._touch("truth value")

    def __eq__(self, other):
        return self._touch("comparison")

    def __ne__(self, other):
        return self._touch("comparison")

    def __lt__(self, other):
        return self._touch("comparison")

    def __gt__(self, other):
        return self._touch("comparison")

    def __hash__(self):
        return self._touch("hash")

    def __mro_entries__(self, bases):
        return self._touch("subclassing")

    def __repr__(self):
        return f"<absent original internal {object.__getattribute__(self, '_name')}>"


def _stub_module(name):
    module = types.ModuleType(name)
    module.__absent_original__ = True

    def __getattr__(attr):
        if attr.startswith("__"):
            raise AttributeError(attr)
        INJECTED.setdefault(f"{name}.{attr}", "module-missing")
        return Absent(f"{name}.{attr}")

    module.__getattr__ = __getattr__
    module.__path__ = []  # allow submodule stubs below it
    sys.modules[name] = module
    parent, _, child = name.rpartition(".")
    if parent:
        setattr(_ensure(parent), child, module)
    return module


def _ensure(name):
    if name in sys.modules:
        return sys.modules[name]
    try:
        return importlib.import_module(name)
    except ModuleNotFoundError as error:
        if error.name is None or not (name == error.name or name.startswith(error.name + ".") or error.name.startswith(name)):
            raise
        if name == "chartgen":
            raise
        return _stub_module(name)


def _imports(path):
    """(module, name-or-None) for every chartgen import anywhere in the test file."""
    tree = ast.parse(Path(path).read_text(encoding="utf-8"))
    out = []
    for node in ast.walk(tree):
        if isinstance(node, ast.ImportFrom) and node.module and node.module.split(".")[0] == "chartgen" and node.level == 0:
            out += [(node.module, alias.name) for alias in node.names]
        elif isinstance(node, ast.Import):
            out += [(alias.name, None) for alias in node.names if alias.name.split(".")[0] == "chartgen"]
    return out


def install_placeholders(test_file):
    for module, name in _imports(test_file):
        mod = _ensure(module)
        if name is None or hasattr(mod, name):
            continue
        try:  # a submodule the rewrite provides
            importlib.import_module(f"{module}.{name}")
            continue
        except ModuleNotFoundError:
            pass
        if getattr(mod, "__absent_original__", False):
            INJECTED.setdefault(f"{module}.{name}", "module-missing")
        else:
            INJECTED.setdefault(f"{module}.{name}", "name-missing")
        setattr(mod, name, Absent(f"{module}.{name}"))


class Recorder(unittest.TestResult):
    def __init__(self):
        super().__init__()
        self.records = {}
        self.fixture_touches = []
        self.class_native_calls = {}
        self._start = None

    def _rec(self, test):
        tid = test.id()
        return self.records.setdefault(tid, {"id": tid, "outcome": "pass", "subtests": [], "details": [],
                                             "touched": []})

    def startTest(self, test):
        super().startTest(test)
        if TOUCHED:  # touches outside any test: class/module fixtures
            self.fixture_touches += [(type(test).__name__, *t) for t in TOUCHED]
            TOUCHED.clear()
        self._start = time.monotonic()
        cls = type(test).__name__   # calls before a test starts: its class's setUpClass (or module import)
        self.class_native_calls[cls] = self.class_native_calls.get(cls, 0) + NATIVE_CALLS[0]
        NATIVE_CALLS[0] = 0
        self._rec(test)

    def stopTest(self, test):
        r = self._rec(test)
        r["seconds"] = round(time.monotonic() - self._start, 3)
        r["touched"] = sorted({t[0] for t in TOUCHED})
        r["native_calls"] = NATIVE_CALLS[0]
        NATIVE_CALLS[0] = 0
        r["touch_sites"] = sorted({f"{t[0]} [{t[1]}] at {t[2]}" for t in TOUCHED})[:8]
        TOUCHED.clear()
        super().stopTest(test)

    def _bad(self, test, err, outcome):
        if isinstance(test, unittest.case._SubTest):
            r = self._rec(test.test_case)
            r["subtests"].append({"params": test._subDescription(), "outcome": outcome,
                                  "detail": self._detail(err, test.test_case)})
            if r["outcome"] in ("pass", "fail") and outcome == "error":
                r["outcome"] = "error"
            elif r["outcome"] == "pass":
                r["outcome"] = outcome
            return
        if not hasattr(test, "_testMethodName"):   # an _ErrorHolder: setUpClass/tearDown/module import failure
            desc = str(test)
            self.records.setdefault(f"<fixture> {desc}", {"id": f"<fixture> {desc}", "outcome": "error",
                                                          "details": [self._detail(err, None)], "subtests": [],
                                                          "touched": sorted({t[0] for t in TOUCHED})})
            if TOUCHED:
                self.fixture_touches += [(desc, *t) for t in TOUCHED]
                TOUCHED.clear()
            return
        r = self._rec(test)
        r["outcome"] = outcome if r["outcome"] != "error" else "error"
        r["details"].append(self._detail(err, test))

    def _detail(self, err, test):
        etype, value, tb = err
        text = "".join(traceback.format_exception(etype, value, tb, limit=-4))
        return {"type": etype.__name__, "message": str(value)[:600], "traceback": text[-2500:]}

    def addError(self, test, err):
        self._bad(test, err, "error")

    def addFailure(self, test, err):
        self._bad(test, err, "fail")

    def addSubTest(self, test, subtest, err):
        if err is not None:
            self._bad(subtest, err, "fail" if issubclass(err[0], test.failureException) else "error")

    def addSkip(self, test, reason):
        r = self._rec(test)
        r["outcome"] = "skip"
        r["details"].append({"type": "skip", "message": reason})

    def addExpectedFailure(self, test, err):
        self._rec(test)["outcome"] = "expected-failure"

    def addUnexpectedSuccess(self, test):
        self._rec(test)["outcome"] = "unexpected-success"


def main():
    stage, module, out = Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3])
    impl = sys.argv[sys.argv.index("--impl") + 1]
    sys.path.insert(0, str(stage))
    # The original's chartgen puts the shared tools (bake.py, arrowheads.py, figkit) on the path and its
    # tests import them as consumers; they are not chartgen. Appended, so they never shadow the package.
    sys.path.append(str(stage.parent / "tools"))
    import chartgen
    chart_file = Path(chartgen.__file__).resolve()
    if impl == "rewrite" and str(chart_file).startswith(str(stage.resolve())):
        raise SystemExit("rewrite mode imported the original chartgen from the stage")
    if impl == "oracle" and not str(chart_file).startswith(str(stage.resolve())):
        raise SystemExit(f"oracle mode imported chartgen from {chart_file}, not the stage")
    test_file = stage / "tests" / f"{module}.py"
    import_error = None
    if impl == "rewrite":
        install_placeholders(test_file)
        # Count calls into the Rust engine per test: a pass with none exercised only host Python
        # (for example the original spec.py the rewrite ships as its host-side reader).
        native = importlib.import_module("chartgen._native")
        dispatch = native.dispatch

        def counted(*args, **kwargs):
            NATIVE_CALLS[0] += 1
            return dispatch(*args, **kwargs)
        native.dispatch = counted
    started = time.monotonic()
    result = Recorder()
    try:
        suite = unittest.defaultTestLoader.loadTestsFromName(f"tests.{module}")
    except Exception:  # recorded, never hidden
        import_error = traceback.format_exc(limit=-6)
        suite = None
    module_touches = sorted({t[0] for t in TOUCHED})
    TOUCHED.clear()
    if suite is not None:
        suite.run(result)
    if result.fixture_touches or TOUCHED:
        result.fixture_touches += [("<after last test>", *t) for t in TOUCHED]
    data = {"module": module, "impl": impl, "chartgen": str(chart_file), "import_error": import_error,
            "module_touches": module_touches, "injected": INJECTED,
            "fixture_touches": sorted({f"{c}: {n}" for c, n, *_ in result.fixture_touches}),
            "loader_errors": [str(e)[-2500:] for e in getattr(unittest.defaultTestLoader, "errors", [])],
            "class_native_calls": result.class_native_calls,
            "seconds": round(time.monotonic() - started, 2), "tests": list(result.records.values())}
    out.write_text(json.dumps(data, indent=1, ensure_ascii=False, default=str) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
