#!/usr/bin/env python3
"""Wait for all required main push workflows for one exact commit SHA."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
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

RESOURCE_EVIDENCE_SCHEMA = "xp-history-throughput-v1"


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


def validate_resource_evidence(raw: str | None, target_sha: str) -> str:
    """Require a resource attestation before any release artifact is published."""
    if not raw:
        raise ReadinessError("resource_evidence_missing")
    try:
        evidence = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise ReadinessError("resource_evidence_invalid_json") from exc
    if not isinstance(evidence, dict):
        raise ReadinessError("resource_evidence_invalid_shape")
    if evidence.get("schema") != RESOURCE_EVIDENCE_SCHEMA:
        raise ReadinessError("resource_evidence_schema_mismatch")
    if evidence.get("target_sha") != target_sha:
        raise ReadinessError("resource_evidence_sha_mismatch")
    log_sha = evidence.get("log_sha")
    if not isinstance(log_sha, str) or not re.fullmatch(r"[0-9a-f]{64}", log_sha):
        raise ReadinessError("resource_evidence_log_sha_invalid")
    if evidence.get("window_seconds") != 300:
        raise ReadinessError("resource_evidence_window_invalid")
    if evidence.get("memory_limit_mib") != 128 or evidence.get("swap_limit_mib") != 0:
        raise ReadinessError("resource_evidence_resource_limit_invalid")
    required_gates = (
        "source_ack_meets_ten_x_segments",
        "source_ack_meets_ten_x_bytes",
        "repository_apply_meets_ten_x_segments",
        "repository_apply_meets_ten_x_bytes",
    )
    if any(evidence.get(gate) is not True for gate in required_gates):
        raise ReadinessError("resource_evidence_ten_x_gate_failed")
    return f"resource_evidence_verified:{log_sha}"


def validate_resource_evidence_files(
    evidence_path: str | os.PathLike[str],
    log_path: str | os.PathLike[str],
    target_sha: str,
) -> str:
    """Validate the attestation and the immutable workload log it names."""
    evidence_file = Path(evidence_path)
    log_file = Path(log_path)
    try:
        raw = evidence_file.read_text(encoding="utf-8")
        log_bytes = log_file.read_bytes()
    except OSError as exc:
        raise ReadinessError("resource_evidence_files_unreadable") from exc
    detail = validate_resource_evidence(raw, target_sha)
    evidence = json.loads(raw)
    actual_log_sha = hashlib.sha256(log_bytes).hexdigest()
    if evidence["log_sha"] != actual_log_sha:
        raise ReadinessError("resource_evidence_log_sha_mismatch")
    log_text = log_bytes.decode("utf-8", errors="replace")
    if "test result: ok" not in log_text or "source_journal_resource" not in log_text:
        raise ReadinessError("resource_evidence_log_incomplete")
    if re.search(r"^test result: FAILED\b", log_text, re.MULTILINE) or re.search(
        r"^test [^\n]+ \.\.\. FAILED\b", log_text, re.MULTILINE
    ):
        raise ReadinessError("resource_evidence_workload_failed")
    for workload in (
        "xp_source_delivery_journal_memory_e2e",
        "xp_repository_summary_memory_e2e",
    ):
        if not re.search(
            rf"^test {re.escape(workload)} \.\.\. ok$", log_text, re.MULTILINE
        ):
            raise ReadinessError(f"resource_evidence_workload_incomplete:{workload}")
    metric_line = next(
        (
            line
            for line in log_text.splitlines()
            if line.startswith("source_journal_resource ")
        ),
        None,
    )
    if metric_line is None:
        raise ReadinessError("resource_evidence_metrics_missing")
    metric_fields = (
        "sample_window_seconds",
        "generated_segments",
        "generated_bytes",
        "peak_generated_segments",
        "peak_generated_bytes",
        "acknowledged_segments",
        "acknowledged_bytes",
        "applied_segments",
        "applied_bytes",
    )
    metrics: dict[str, int] = {}
    for field in metric_fields:
        match = re.search(rf"(?:^|\s){re.escape(field)}=(\d+)(?:\s|$)", metric_line)
        if match is None:
            raise ReadinessError(f"resource_evidence_metric_missing:{field}")
        value = int(match.group(1))
        evidence_value = evidence.get(field)
        if not isinstance(evidence_value, int) or isinstance(evidence_value, bool):
            raise ReadinessError(f"resource_evidence_metric_invalid:{field}")
        if evidence_value != value:
            raise ReadinessError(f"resource_evidence_metric_mismatch:{field}")
        metrics[field] = value
    if metrics["sample_window_seconds"] < 300:
        raise ReadinessError("resource_evidence_window_observed_invalid")
    if metrics["peak_generated_segments"] == 0 or metrics["peak_generated_bytes"] == 0:
        raise ReadinessError("resource_evidence_generation_missing")
    computed_gates = {
        "source_ack_meets_ten_x_segments": metrics["acknowledged_segments"]
        >= metrics["peak_generated_segments"] * 10,
        "source_ack_meets_ten_x_bytes": metrics["acknowledged_bytes"]
        >= metrics["peak_generated_bytes"] * 10,
        "repository_apply_meets_ten_x_segments": metrics["applied_segments"]
        >= metrics["peak_generated_segments"] * 10,
        "repository_apply_meets_ten_x_bytes": metrics["applied_bytes"]
        >= metrics["peak_generated_bytes"] * 10,
    }
    for field, computed in computed_gates.items():
        if evidence.get(field) is not computed:
            raise ReadinessError(f"resource_evidence_gate_mismatch:{field}")
    return detail


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
    parser.add_argument("--resource-evidence")
    parser.add_argument("--resource-evidence-file")
    parser.add_argument("--resource-log-file")
    args = parser.parse_args()
    if not args.sha or len(args.sha) != 40:
        print("release-readiness: invalid target SHA", file=sys.stderr)
        return 1
    try:
        if args.resource_evidence_file or args.resource_log_file:
            if not args.resource_evidence_file or not args.resource_log_file:
                raise ReadinessError("resource_evidence_files_incomplete")
            resource_detail = validate_resource_evidence_files(
                args.resource_evidence_file,
                args.resource_log_file,
                args.sha,
            )
        else:
            resource_detail = validate_resource_evidence(args.resource_evidence, args.sha)
    except ReadinessError as exc:
        print(f"release-readiness: {exc}", file=sys.stderr)
        return 1
    print(f"release-readiness: {resource_detail}")

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
