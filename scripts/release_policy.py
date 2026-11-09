"""Conventional Commit release planning, scoped to Deductum's adopted history."""
import json
from pathlib import Path
import re
import subprocess

TAG = re.compile(r"v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)")
SUBJECT = re.compile(r"(?P<type>[a-z]+)(?:\((?P<scope>[^()\r\n]+)\))?(?P<breaking>!)?: (?P<description>\S.*)", re.I)
FOOTER = re.compile(r"^BREAKING[ -]CHANGE: \S", re.M)
# A footer that sets the next version by hand: the only way to leave 0.x for 1.0.0
RELEASE_AS = re.compile(r"^Release-As: v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)[ \t]*$", re.M | re.I)


def git(*args):
    return subprocess.check_output(["git", *args], text=True).strip()


def ancestor(older, newer):
    result = subprocess.run(["git", "merge-base", "--is-ancestor", older, newer], capture_output=True)
    if result.returncode not in (0, 1):
        raise ValueError("Release history is incomplete or invalid")
    return result.returncode == 0


def change(message):
    subject, _, body = message.partition("\n")
    match = SUBJECT.fullmatch(subject)
    if not match:
        return 0, None
    rank = 3 if match["breaking"] or FOOTER.search(body) else {"feat": 2, "fix": 1, "perf": 1}.get(match["type"].lower(), 0)
    return rank, match["description"]


def bump(base, rank):
    major, minor, patch = base
    # Before 1.0 a breaking change is a minor bump: 1.0.0 is only ever chosen by hand
    if rank == 3 and major == 0:
        rank = 2
    if rank == 3:
        return major + 1, 0, 0
    if rank == 2:
        return major, minor + 1, 0
    if rank == 1:
        return major, minor, patch + 1
    return None


def requested(messages):
    """Versions asked for with a `Release-As: X.Y.Z` footer (the subject line does not count)"""
    return [tuple(map(int, match.groups()))
            for message in messages for match in RELEASE_AS.finditer(message.partition("\n")[2])]


def next_version(base, messages):
    rank = max((change(message)[0] for message in messages), default=0)
    automatic = bump(base, rank)
    # A requested version releases even after only docs or chores; one at or below the
    # base cannot be honoured and is ignored, as is one below the automatic bump
    manual = [version for version in requested(messages) if version > base]
    return max([*manual, *([automatic] if automatic else [])], default=None)


def plan(sha, tags=None):
    config = json.loads(Path("release-config.json").read_text())
    bootstrap = config["bootstrap_sha"]
    if not ancestor(bootstrap, sha):
        raise ValueError("Source revision predates the release-policy adoption point")
    candidates = []
    reserved = []
    if tags is None:
        tags = [(tag, git("rev-parse", tag + "^{commit}"))
                for tag in git("tag", "--merged", sha).splitlines() if TAG.fullmatch(tag)]
    for tag, revision in tags:
        match = TAG.fullmatch(tag)
        # A tag created after this checkout may point to a future commit.
        # Full checkout history contains every ancestor of the candidate.
        exists = subprocess.run(["git", "cat-file", "-e", revision + "^{commit}"], capture_output=True).returncode == 0
        if match and exists and revision != bootstrap and ancestor(bootstrap, revision) and ancestor(revision, sha):
            version = tuple(map(int, match.groups()))
            if revision == sha:
                reserved.append((version, tag))
            else:
                candidates.append((version, revision))
    base, start = max(candidates) if candidates else (tuple(map(int, config["initial_version"].split("."))), bootstrap)
    # Include merged PR commits as well as direct/fast-forward commits. The
    # bootstrap point excludes earlier and vendored history.
    messages = [message for message in git("log", "--reverse", "--no-merges", "--format=%B%x00", f"{start}..{sha}").split("\0") if message.strip()]
    messages = [message.strip() for message in messages]
    version = next_version(base, messages)
    if version is None and not reserved:
        return None
    groups = {3: ("Breaking changes", []), 2: ("Features", []), 1: ("Fixes and performance", [])}
    for message in messages:
        rank, description = change(message)
        if rank:
            groups[rank][1].append(description)
    notes = "\n\n".join(heading + "\n" + "\n".join("- " + item for item in items)
                          for heading, items in groups.values() if items)
    tag = max(reserved)[1] if reserved else "v" + ".".join(map(str, version))
    if not notes and not reserved:
        notes = f"Released as {tag}, set by a Release-As footer, with no listed features or fixes."
    return {"tag": tag, "notes": notes, "reserved": bool(reserved)}
