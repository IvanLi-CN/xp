"""Merge gate reads live PR labels. It never changes the measurement job."""

import json
import os
import sys
import time
from github import api, pages
from policy import gate

repo = os.environ["GITHUB_REPOSITORY"]
if os.environ["APPLICABLE"] not in {"true", "false"}:
    raise ValueError("classification unavailable; cannot infer not applicable")


def latest_measurement(sha):
    if os.environ["EXECUTE"] == "true":
        return os.environ["MEASUREMENT"]
    runs = pages(
        f"repos/{repo}/actions/workflows/xp-proxy-perf.yml/runs?event=pull_request&head_sha={sha}",
        "workflow_runs",
    )
    for run in sorted(runs, key=lambda r: r["id"], reverse=True):
        if run["id"] == int(os.environ["GITHUB_RUN_ID"]):
            continue
        jobs = pages(
            f"repos/{repo}/actions/runs/{run['id']}/attempts/{run['run_attempt']}/jobs",
            "jobs",
        )
        # Label-only runs do not execute the quality/build/measurement pipeline.
        quality = next((j for j in jobs if j["name"] == "protocol-quality"), None)
        if quality is None or quality["conclusion"] == "skipped":
            continue
        measurement = next((j for j in jobs if j["name"] == "measurement"), None)
        if measurement and measurement["conclusion"] != "skipped":
            return measurement["conclusion"] or "pending"
        if run["status"] == "completed":
            return "error"
        return "pending"
    return "pending"


deadline = time.monotonic() + 900
while True:
    pr = api(f"repos/{repo}/pulls/{int(os.environ['PR_NUMBER'])}")
    sha = pr["head"]["sha"]
    if sha != os.environ["EXPECTED_SHA"]:
        raise ValueError("stale head: cannot satisfy current candidate")
    measurement = latest_measurement(sha)
    state, description = gate(
        applicable=os.environ["APPLICABLE"] == "true",
        waived=any(label["name"] == "perf:waive" for label in pr["labels"]),
        measurement=measurement,
        current_sha=sha,
        measured_sha=os.environ["EXPECTED_SHA"],
    )
    if state != "pending" or time.monotonic() >= deadline:
        break
    # Keep the Check in progress while the actual automatic run is still running.
    # No producer restart and no measurement cancellation occurs here.
    time.sleep(10)

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
