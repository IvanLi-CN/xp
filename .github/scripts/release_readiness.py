#!/usr/bin/env python3
"""Wait for all required main push workflows for one exact commit SHA."""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
from dataclasses import dataclass
from typing import Any
from urllib import error, request

REQUIRED_WORKFLOWS = ("ci", "fixture-policy", "xray-e2e")
FAILURE_CONCLUSIONS = {
    "failure",
    "cancelled",
    "skipped",
    "timed_out",
    "action_required",
    "stale",
    "neutral",
}


class ReadinessError(RuntimeError):
    """A fail-closed readiness error."""


@dataclass(frozen=True)
class WorkflowState:
    name: str
    status: str
    conclusion: str
    run_id: str


class GitHubApi:
    def __init__(self, root: str, token: str):
        self.root = root.rstrip("/")
        self.token = token

    def get_json(self, path: str) -> Any:
        url = path if path.startswith("http") else f"{self.root}{path}"
        req = request.Request(
            url,
            headers={
                "Accept": "application/vnd.github+json",
                "Authorization": f"Bearer {self.token}",
                "X-GitHub-Api-Version": "2022-11-28",
                "User-Agent": "xp-release-readiness",
            },
        )
        try:
            with request.urlopen(req, timeout=20) as response:
                return json.loads(response.read().decode("utf-8"))
        except (OSError, ValueError, error.HTTPError) as exc:
            raise ReadinessError(f"github_api_failure:{path}:{type(exc).__name__}") from exc


def latest_runs(payload: Any, target_sha: str) -> dict[str, WorkflowState]:
    if isinstance(payload, dict):
        runs = payload.get("workflow_runs", [])
    else:
        runs = payload
    if not isinstance(runs, list):
        raise ReadinessError("github_api_shape_failure:workflow_runs")

    selected: dict[str, dict[str, Any]] = {}
    for run in runs:
        if not isinstance(run, dict):
            continue
        name = run.get("name")
        if name not in REQUIRED_WORKFLOWS:
            continue
        if run.get("head_sha") != target_sha or run.get("event") != "push":
            continue
        if run.get("head_branch") != "main":
            continue
        current = selected.get(name)
        current_key = (
            str(current.get("created_at", "")) if current else "",
            int(current.get("id", 0)) if current else 0,
        )
        next_key = (str(run.get("created_at", "")), int(run.get("id", 0) or 0))
        if current is None or next_key > current_key:
            selected[name] = run

    return {
        name: WorkflowState(
            name=name,
            status=str(run.get("status") or "unknown"),
            conclusion=str(run.get("conclusion") or ""),
            run_id=str(run.get("id") or "unknown"),
        )
        for name, run in selected.items()
    }


def evaluate(payload: Any, target_sha: str) -> tuple[str, str]:
    states = latest_runs(payload, target_sha)
    diagnostics = []
    for name in REQUIRED_WORKFLOWS:
        state = states.get(name)
        if state is None:
            diagnostics.append(f"{name}:missing")
        else:
            conclusion = state.conclusion or "none"
            diagnostics.append(f"{name}:{state.status}/{conclusion}:{state.run_id}")

    failures = [
        name
        for name in REQUIRED_WORKFLOWS
        if name in states
        and states[name].status == "completed"
        and states[name].conclusion in FAILURE_CONCLUSIONS
    ]
    if failures:
        return "failed", "; ".join(diagnostics)

    pending = [
        name
        for name in REQUIRED_WORKFLOWS
        if name not in states or states[name].status != "completed"
    ]
    if pending:
        return "pending", "; ".join(diagnostics)

    non_success = [
        name
        for name in REQUIRED_WORKFLOWS
        if states[name].conclusion != "success"
    ]
    if non_success:
        return "failed", "; ".join(diagnostics)
    return "ready", "all_required_workflows_success"


def load_payload(api: GitHubApi, repository: str, fixture: str | None) -> Any:
    if fixture:
        with open(fixture, encoding="utf-8") as handle:
            return json.load(handle)
    return api.get_json(
        f"/repos/{repository}/actions/runs?event=push&branch=main&per_page=100"
    )


def run() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repository", default=os.environ.get("GITHUB_REPOSITORY", ""))
    parser.add_argument("--token", default=os.environ.get("GITHUB_TOKEN", ""))
    parser.add_argument("--api-root", default=os.environ.get("GITHUB_API_URL", "https://api.github.com"))
    parser.add_argument("--sha", default=os.environ.get("RELEASE_HEAD_SHA", ""))
    parser.add_argument("--fixture")
    args = parser.parse_args()
    if not args.sha or len(args.sha) != 40:
        print("release-readiness: invalid target SHA", file=sys.stderr)
        return 1

    timeout = max(0, int(os.environ.get("RELEASE_READINESS_TIMEOUT_SECONDS", "1800")))
    interval = max(1, int(os.environ.get("RELEASE_READINESS_POLL_SECONDS", "10")))
    deadline = time.monotonic() + timeout
    api = GitHubApi(args.api_root, args.token)

    while True:
        try:
            payload = load_payload(api, args.repository, args.fixture)
            status, detail = evaluate(payload, args.sha)
        except (OSError, ValueError, ReadinessError) as exc:
            print(f"release-readiness: {exc}", file=sys.stderr)
            return 1
        print(f"release-readiness: status={status} sha={args.sha} {detail}")
        if status == "ready":
            return 0
        if status == "failed":
            return 1
        if args.fixture or time.monotonic() >= deadline:
            print(f"release-readiness: timeout sha={args.sha} {detail}", file=sys.stderr)
            return 1
        time.sleep(min(interval, max(0.1, deadline - time.monotonic())))


if __name__ == "__main__":
    raise SystemExit(run())
