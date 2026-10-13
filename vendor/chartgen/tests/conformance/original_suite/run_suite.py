"""Run the original chartgen unittest suite against the rewrite, and against the original as a control.

    python run_suite.py --oracle-root ORIGINAL/charts --work SCRATCH --impl rewrite \
        [--impl oracle --oracle-python ORACLE_VENV/bin/python] [--only test_cli] [--jobs 2]

The original test modules are read from ``ORIGINAL/charts/tests`` and verified
byte-for-byte against ``tests/conformance/oracle-source.sha256``; nothing in the
original tree is written. Each implementation gets its own stage under
``SCRATCH/<impl>/stage``:

    charts/tests/        copies of the original test modules and fixtures
    charts/examples      link to the bundled tests/conformance/examples
    charts/style/        copy of the original stylesheets (tests @import them)
    tools/               copy of the original shared tools (bake.py, figkit): consumers, not chartgen
    charts/chartgen/     oracle only: a copy of the original package (its measurement cache lands
                         in the stage, never in the original tree)

The rewrite stage holds no ``chartgen`` package, so the tests import the
rewrite's installed binding (python/chartgen). Each module runs in its own
process through _harness.py. Raw per-module results go to
``SCRATCH/<impl>/raw/<module>.json``; classify.py turns both into the result
table.
"""
import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
CONFORMANCE = HERE.parent
MANIFEST = CONFORMANCE / "oracle-source.sha256"


def manifest():
    out = {}
    for line in MANIFEST.read_text().splitlines():
        digest, path = line.split(None, 1)
        out[path.strip()] = digest
    return out


def verify(oracle_root):
    """The original files the stage copies, checked against the bundled manifest."""
    share = Path(oracle_root).resolve().parent          # the manifest is relative to charts/..
    sums, bad, checked = manifest(), [], 0
    for rel, digest in sums.items():
        if not (rel.startswith("charts/tests/") or rel.startswith("charts/chartgen/")
                or rel.startswith("charts/style/") or rel.startswith("tools/")):
            continue
        path = share / rel
        if not path.exists() or hashlib.sha256(path.read_bytes()).hexdigest() != digest:
            bad.append(rel)
        checked += 1
    if bad:
        raise SystemExit(f"original tree differs from oracle-source.sha256: {bad[:10]}")
    return checked


def _copy(src, dst, pattern_ok=lambda p: True):
    for path in sorted(src.rglob("*")):
        if "__pycache__" in path.parts or path.is_dir() or not pattern_ok(path):
            continue
        target = dst / path.relative_to(src)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, target)


def stage(oracle_root, work, impl):
    root = work / impl / "stage"
    if root.exists():
        shutil.rmtree(root)
    charts = root / "charts"
    _copy(oracle_root / "tests", charts / "tests")
    (charts / "examples").parent.mkdir(parents=True, exist_ok=True)
    os.symlink(CONFORMANCE / "examples", charts / "examples")
    _copy(oracle_root / "style", charts / "style")
    _copy(oracle_root.parent / "tools", root / "tools")
    if impl == "oracle":
        _copy(oracle_root / "chartgen", charts / "chartgen")
        cache = oracle_root / ".cache" / "measure.json"
        if cache.exists():  # the original's own Typst measurements (keyed by Typst version); a warm start
            (charts / ".cache").mkdir()
            shutil.copy2(cache, charts / ".cache" / "measure.json")
    return charts


def modules(charts):
    return sorted(p.stem for p in (charts / "tests").glob("test_*.py"))


def run_module(python, charts, module, raw, impl, timeout):
    out = raw / f"{module}.json"
    env = dict(os.environ, PYTHONDONTWRITEBYTECODE="1", PYTHONHASHSEED="0")
    env.pop("PYTHONPATH", None)
    started = time.monotonic()
    try:
        proc = subprocess.run([python, str(HERE / "_harness.py"), str(charts), module, str(out), "--impl", impl],
                              cwd=charts, env=env, capture_output=True, text=True, timeout=timeout)
        status = proc.returncode
        stderr = proc.stderr[-4000:]
    except subprocess.TimeoutExpired as error:
        status, stderr = "timeout", str(error)
    if status != 0 or not out.exists():   # the harness itself failed: recorded, never hidden
        out.write_text(json.dumps({"module": module, "impl": impl, "harness_failure": {"status": status,
                                   "stderr": stderr}, "tests": []}, indent=1) + "\n")
    return module, status, round(time.monotonic() - started, 1)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--oracle-root", type=Path, required=True, help="the original charts/ directory")
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--impl", action="append", choices=("rewrite", "oracle"), required=True)
    parser.add_argument("--oracle-python", default=None, help="python with the original's dependencies")
    parser.add_argument("--rewrite-python", default=sys.executable, help="python with the rewrite binding installed")
    parser.add_argument("--only", action="append", default=[])
    parser.add_argument("--jobs", type=int, default=2)
    parser.add_argument("--timeout", type=int, default=3600)
    args = parser.parse_args()
    oracle_root = args.oracle_root.resolve()
    checked = verify(oracle_root)
    for impl in args.impl:
        python = args.oracle_python if impl == "oracle" else args.rewrite_python
        if python is None:
            raise SystemExit("--oracle-python is required for --impl oracle")
        charts = stage(oracle_root, args.work.resolve(), impl)
        raw = args.work.resolve() / impl / "raw"
        raw.mkdir(parents=True, exist_ok=True)
        names = [m for m in modules(charts) if not args.only or m in args.only]
        with ThreadPoolExecutor(max_workers=args.jobs) as pool:
            for module, status, seconds in pool.map(lambda m: run_module(python, charts, m, raw, impl, args.timeout),
                                                    names):
                print(f"{impl:8s} {module:24s} harness-exit={status} {seconds}s", flush=True)
        (raw.parent / "_run.json").write_text(json.dumps({
            "impl": impl, "python": python, "oracle_root": str(oracle_root), "manifest_files_checked": checked,
            "modules": names}, indent=1) + "\n")


if __name__ == "__main__":
    main()
