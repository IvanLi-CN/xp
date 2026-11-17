"""Merge gate reads live PR labels. It never changes the measurement job."""

import json
import os
import sys
from github import api, pages
from policy import gate

repo = os.environ["GITHUB_REPOSITORY"]
pr = api(f"repos/{repo}/pulls/{int(os.environ['PR_NUMBER'])}")
sha = pr["head"]["sha"]
if sha != os.environ["EXPECTED_SHA"]:
    print("stale head: cannot satisfy current candidate")
    sys.exit(1)
if os.environ["APPLICABLE"] not in {"true", "false"}:
    raise ValueError("classification unavailable; cannot infer not applicable")
measurement = os.environ["MEASUREMENT"]
if os.environ["EXECUTE"] != "true":
    runs = pages(
        f"repos/{repo}/actions/workflows/xp-proxy-perf.yml/runs?event=pull_request&head_sha={sha}",
        "workflow_runs",
    )
    # Label-only runs must not consume another label gate, a manual full, or an old attempt.
    runs = [r for r in runs if r["id"] != int(os.environ["GITHUB_RUN_ID"])]
    measurement = "pending"
    for run in sorted(runs, key=lambda r: r["id"], reverse=True):
        jobs = pages(
            f"repos/{repo}/actions/runs/{run['id']}/attempts/{run['run_attempt']}/jobs",
            "jobs",
        )
        job = next(
            (
                j
                for j in jobs
                if j["name"] == "measurement" and j["conclusion"] != "skipped"
            ),
            None,
        )
        if job:
            measurement = job["conclusion"] or "pending"
            break
state, description = gate(
    applicable=os.environ["APPLICABLE"] == "true",
    waived=any(label["name"] == "perf:waive" for label in pr["labels"]),
    measurement=measurement,
    current_sha=sha,
    measured_sha=os.environ["EXPECTED_SHA"],
)
print(
    json.dumps(
        dict(
            merge_gate=state, description=description, measurement=measurement, sha=sha
        )
    )
)
with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as summary:
    summary.write(
        f"{description}\n\nRaw measurement: {measurement}. Tests are never skipped by perf:waive.\n"
    )
sys.exit(0 if state == "success" else 1)
