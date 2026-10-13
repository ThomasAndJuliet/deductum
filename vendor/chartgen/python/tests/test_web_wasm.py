"""The browser build as WebAssembly: wasm-bindgen glue generated for the built
chartgen_web module with the CLI version Cargo.lock pins, then exercised in Node
(packages/web/wasm.test.mjs) together with the host helper's own tests."""
import os
import re
import subprocess
from pathlib import Path

from conftest import require

ROOT = Path(__file__).resolve().parents[2]
WASM = ROOT / "target" / "wasm32-unknown-unknown" / "debug" / "chartgen_web.wasm"


def locked_wasm_bindgen():
    lock = (ROOT / "Cargo.lock").read_text()
    return re.search(r'name = "wasm-bindgen"\nversion = "([^"]+)"', lock).group(1)


def test_browser_wasm_end_to_end_in_node(tmp_path):
    require("node", "Node.js")
    require("wasm-bindgen", f"cargo install wasm-bindgen-cli --version {locked_wasm_bindgen()} --locked")
    require(str(WASM), "cargo build -p chartgen-web --target wasm32-unknown-unknown")
    version = subprocess.run(["wasm-bindgen", "--version"], capture_output=True, text=True, check=True).stdout.split()[-1]
    assert version == locked_wasm_bindgen(), "the wasm-bindgen CLI must match the crate version in Cargo.lock"
    subprocess.run(["wasm-bindgen", "--target", "nodejs", "--out-dir", str(tmp_path), str(WASM)], check=True)
    env = {**os.environ, "CHARTGEN_WEB_GLUE": str(tmp_path / "chartgen_web.js")}
    run = subprocess.run(["node", "--test", "packages/web/wasm.test.mjs", "packages/web/chartgen.test.mjs"],
                         cwd=ROOT, env=env, capture_output=True, text=True)
    assert run.returncode == 0, run.stdout[-3000:] + run.stderr[-2000:]
    assert re.search(r"# pass (\d+)", run.stdout).group(1) == "9" and "# fail 0" in run.stdout
