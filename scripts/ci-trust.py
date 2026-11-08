#!/usr/bin/env python3
"""Decide what a CI run may use, from GitHub's own description of the event.

Only a run in the configured repository, from that repository's own code, is
trusted, and only a trusted run uses the household runners: their shared caches
treat every job as household code.

This keeps honest workflows honest. A fork's pull request can rewrite this file
and the workflow, so the boundary against hostile code is the runner group and
the absence of secrets in fork runs, never this script. See the README.
"""
import json
import os
from pathlib import Path

import release_config

HOSTED = "ubuntu-24.04"
HOUSEHOLD = "neo-ubuntu"
CI_EVENTS = {"push", "pull_request", "workflow_dispatch"}


def classify(event_name, event, repository, expected, ci_runner=""):
    base = event.get("repository") or {}
    own_code = True
    if event_name == "pull_request":
        # A deleted fork leaves head.repo null, which is not this repository.
        head = ((event.get("pull_request") or {}).get("head") or {}).get("repo") or {}
        own_code = head.get("full_name") == repository
    trusted = (event_name in CI_EVENTS and repository == expected
               and base.get("fork") is False and own_code)
    household = trusted and ci_runner != "hosted"
    return {
        "trusted": "true" if trusted else "false",
        "runner": HOUSEHOLD if household else HOSTED,
        "flatpak": "neo" if household else "hosted",
    }


def main():
    event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text(encoding="utf-8"))
    decision = classify(os.environ["GITHUB_EVENT_NAME"], event, os.environ["GITHUB_REPOSITORY"],
                        release_config.repository(), os.environ.get("CI_RUNNER", ""))
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        output.writelines(f"{key}={value}\n" for key, value in decision.items())
    print(json.dumps(decision, sort_keys=True))
    if decision["trusted"] != "true":
        print("::notice::This run uses GitHub-hosted runners only.")


if __name__ == "__main__":
    main()
