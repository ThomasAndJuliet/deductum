#!/usr/bin/env python3
"""Find a successful push run of this workflow file for this commit.

The same commit means the same workflow file, so that run validated exactly
what this run would. Prints run=ID (or run=) to GITHUB_OUTPUT. Any doubt, such
as an API error, means not validated: the workflow then runs in full.
"""
import json
import os
import urllib.parse
import urllib.request


def validated_run(runs, path, sha, current):
    matches = [r for r in runs if r["path"] == path and r["id"] != current
               and r["head_sha"] == sha and r["conclusion"] == "success"]
    return min((r["id"] for r in matches), default=None)


def main():
    run = ""
    try:
        if os.environ["GITHUB_EVENT_NAME"] == "push":
            repository = os.environ["GITHUB_REPOSITORY"]
            # owner/repo/.github/workflows/ci.yml@refs/heads/main
            path = os.environ["GITHUB_WORKFLOW_REF"].split("@", 1)[0].split("/", 2)[2]
            query = urllib.parse.urlencode({"head_sha": os.environ["GITHUB_SHA"], "event": "push",
                                            "status": "success", "per_page": 100})
            request = urllib.request.Request(f"https://api.github.com/repos/{repository}/actions/runs?{query}", headers={
                "Accept": "application/vnd.github+json", "Authorization": "Bearer " + os.environ["GITHUB_TOKEN"],
                "X-GitHub-Api-Version": "2022-11-28", "User-Agent": "already-validated"})
            with urllib.request.urlopen(request, timeout=30) as response:
                runs = json.load(response)["workflow_runs"]
            found = validated_run(runs, path, os.environ["GITHUB_SHA"], int(os.environ["GITHUB_RUN_ID"]))
            if found is not None:
                run = str(found)
                print(f"Validated in run {run}: skipping")
            else:
                print("Not validated before: running in full")
    except Exception as error:  # noqa: BLE001 - any doubt means run in full
        print(f"::warning::Could not check for an earlier validation ({error}); running in full")
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        output.write(f"run={run}\n")


if __name__ == "__main__":
    main()
