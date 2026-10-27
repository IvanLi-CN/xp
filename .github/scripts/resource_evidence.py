#!/usr/bin/env python3
"""Build a release resource attestation from the GitHub Actions workload log."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path


METRIC_FIELDS = (
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
WORKLOADS = (
    "xp_source_delivery_journal_memory_e2e",
    "xp_repository_summary_memory_e2e",
)


def fail(message: str) -> None:
    raise SystemExit(message)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target-sha", required=True)
    parser.add_argument("--log", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()

    if not re.fullmatch(r"[0-9a-f]{40}", args.target_sha):
        fail("target SHA must be a 40-character lowercase hexadecimal value")
    try:
        log_bytes = args.log.read_bytes()
    except OSError as exc:
        fail(f"cannot read workload log: {exc}")
    log_text = log_bytes.decode("utf-8", errors="replace")
    if "test result: ok" not in log_text or "source_journal_resource" not in log_text:
        fail("workload log is incomplete")
    if re.search(r"^test result: FAILED\b", log_text, re.MULTILINE):
        fail("workload log contains a failed test result")
    for workload in WORKLOADS:
        if not re.search(rf"^test {re.escape(workload)} \.\.\. ok$", log_text, re.MULTILINE):
            fail(f"workload log is missing successful test: {workload}")

    metric_line = next(
        (line for line in log_text.splitlines() if line.startswith("source_journal_resource ")),
        None,
    )
    if metric_line is None:
        fail("workload log is missing source_journal_resource metrics")
    metrics: dict[str, int] = {}
    for field in METRIC_FIELDS:
        match = re.search(rf"(?:^|\s){re.escape(field)}=(\d+)(?:\s|$)", metric_line)
        if match is None:
            fail(f"workload metrics are missing {field}")
        metrics[field] = int(match.group(1))
    if metrics["sample_window_seconds"] < 300:
        fail("workload sample window is shorter than 300 seconds")
    if metrics["peak_generated_segments"] == 0 or metrics["peak_generated_bytes"] == 0:
        fail("workload generated no measurable source data")

    gates = {
        "source_ack_meets_ten_x_segments": metrics["acknowledged_segments"]
        >= metrics["peak_generated_segments"] * 10,
        "source_ack_meets_ten_x_bytes": metrics["acknowledged_bytes"]
        >= metrics["peak_generated_bytes"] * 10,
        "repository_apply_meets_ten_x_segments": metrics["applied_segments"]
        >= metrics["peak_generated_segments"] * 10,
        "repository_apply_meets_ten_x_bytes": metrics["applied_bytes"]
        >= metrics["peak_generated_bytes"] * 10,
    }
    if not all(gates.values()):
        fail(f"resource throughput gate failed: {gates}")

    evidence = {
        "schema": "xp-history-throughput-v2",
        "target_sha": args.target_sha,
        "log_sha": hashlib.sha256(log_bytes).hexdigest(),
        "window_seconds": 300,
        "memory_limit_mib": 128,
        "swap_limit_mib": 0,
        **metrics,
        **gates,
    }
    args.output.write_text(
        json.dumps(evidence, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
