#!/usr/bin/env python3
"""Test the exact packaging checkout inside the release SDK."""
from pathlib import Path
source = Path(__file__).resolve().parents[1] / 'packaging/flatpak/dev.twright.Deductum.yml'
text = source.read_text()
needle = '  - cargo --offline --locked build --release --bin deductum\n'
assert text.count(needle) == 1
checks = ('  - cargo --offline --locked test --workspace --all-targets --all-features\n'
          '  - python3 -m unittest discover -s host/tests -p "test_*.py"\n'
          '  - python3 scripts/test-publish-release.py\n'
          '  - python3 scripts/test-release-policy.py\n')
env_needle = '    CARGO_HOME: /run/build/deductum/cargo\n'
assert text.count(env_needle) == 1
# The SDK lacks an icon theme; use real symbolic icons for the headless tests.
ci_env = (env_needle + '    XDG_DATA_DIRS: /run/build/deductum/deductum/tests/fixtures/icon-data:/usr/share:/app/share\n'
          '    CARGO_PROFILE_DEV_DEBUG: "0"\n'
          '    CARGO_PROFILE_TEST_DEBUG: "0"\n'
          '    CARGO_INCREMENTAL: "0"\n')
source.with_name('dev.twright.Deductum.ci.yml').write_text(
    text.replace(needle, checks + needle).replace(env_needle, ci_env))
