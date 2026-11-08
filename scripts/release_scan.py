"""Find household names in anything a public release would show.

Findings are rule labels, never the matching text, so a report cannot repeat a
name it found. Package rules look for names that identify the household's
network or machines, and are precise enough to run over binaries. Prose rules
add bare words that are only meaningful in release notes and receipts.
"""
from pathlib import Path
import re
import zipfile

PACKAGE_RULES = {
    "tailnet name": re.compile(rb"\.tail[0-9a-f]{4,}\.ts\.net"),
    "tailnet address": re.compile(rb"(?<![\d.])100\.(?:6[4-9]|[7-9]\d|1[01]\d|12[0-7])\.\d{1,3}\.\d{1,3}(?![\d.])"),
    "household service": re.compile(rb"\bneo:\d{2,5}\b"),
    "household path": re.compile(rb"/mnt/disk-array|/var/lib/neo-infrastructure|/(?:var/)?home/twright\b"),
}
PROSE_RULES = {
    **PACKAGE_RULES,
    "household host": re.compile(rb"\b(?:neo|au-laptop)\b", re.I),
    "household network": re.compile(rb"\b(?:tailnet|tailscale|home-apps)\b", re.I),
}


def findings(data, rules=PACKAGE_RULES):
    return sorted(label for label, pattern in rules.items() if pattern.search(data))


def text_findings(text):
    return findings(text.encode("utf-8"), PROSE_RULES)


def public_lines(text):
    """Remove lines naming household systems; returns the text and how many went."""
    lines = text.splitlines()
    kept = [line for line in lines if not text_findings(line)]
    return "\n".join(kept), len(lines) - len(kept)


def scan_zip(path):
    found = set()
    with zipfile.ZipFile(path) as archive:
        for entry in archive.infolist():
            found.update(findings(entry.filename.encode("utf-8")))
            found.update(findings(archive.read(entry)))
    return sorted(found)


def scan_tree(directory):
    found = set()
    for path in Path(directory).rglob("*"):
        found.update(findings(str(path.relative_to(directory)).encode("utf-8")))
        if path.is_file() and not path.is_symlink():
            found.update(findings(path.read_bytes()))
    return sorted(found)
