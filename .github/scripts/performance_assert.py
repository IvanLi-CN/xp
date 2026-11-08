#!/usr/bin/env python3
"""Fail-closed performance assertions for CI and release workflow jobs."""

from __future__ import annotations

import argparse
from datetime import datetime
import json
from pathlib import Path
import re
from typing import Any


class PerformanceError(RuntimeError):
    pass


CI_PHASE_LIMITS = (("cold", 540), ("warm", 300))


def parse_time(value: Any) -> datetime:
    if not isinstance(value, str) or not value.endswith("Z"):
        raise PerformanceError(f"invalid workflow timestamp: {value!r}")
    return datetime.fromisoformat(value.removesuffix("Z") + "+00:00")


def duration(job: dict[str, Any]) -> float:
    if job.get("conclusion") != "success":
        raise PerformanceError(
            f"job did not succeed: {job.get('name')}={job.get('conclusion')}"
        )
    elapsed = (parse_time(job.get("completed_at")) - parse_time(job.get("started_at"))).total_seconds()
    if elapsed < 0:
        raise PerformanceError(f"negative duration: {job.get('name')}")
    return elapsed


def find_one(jobs: list[dict[str, Any]], pattern: str) -> dict[str, Any]:
    matches = [job for job in jobs if re.fullmatch(pattern, str(job.get("name", "")))]
    if len(matches) != 1:
        names = [job.get("name") for job in matches]
        raise PerformanceError(f"expected one job matching {pattern!r}, got {names}")
    return matches[0]


def find_optional(jobs: list[dict[str, Any]], pattern: str) -> dict[str, Any] | None:
    matches = [job for job in jobs if re.fullmatch(pattern, str(job.get("name", "")))]
    if len(matches) > 1:
        names = [job.get("name") for job in matches]
        raise PerformanceError(f"expected at most one job matching {pattern!r}, got {names}")
    return matches[0] if matches else None


def load_receipts(directory: Path) -> list[dict[str, Any]]:
    receipts = []
    for path in sorted(directory.rglob("*.json")):
        value = json.loads(path.read_text())
        if not isinstance(value, dict):
            raise PerformanceError(f"invalid cache receipt: {path}")
        receipts.append(value)
    return receipts


def workflow_queue_seconds(payload: dict[str, Any]) -> float:
    queued = (parse_time(payload.get("run_started_at")) - parse_time(payload.get("created_at"))).total_seconds()
    if queued < 0:
        raise PerformanceError("negative workflow queue duration")
    return queued


def required_wall_clock_seconds(jobs: list[dict[str, Any]]) -> float:
    if not jobs:
        raise PerformanceError("required jobs payload is empty")
    starts = [parse_time(job.get("started_at")) for job in jobs]
    completions = [parse_time(job.get("completed_at")) for job in jobs]
    elapsed = (max(completions) - min(starts)).total_seconds()
    if elapsed < 0:
        raise PerformanceError("negative required wall-clock duration")
    return elapsed


def retention_seconds(items: list[dict[str, Any]]) -> dict[str, float] | dict[str, str]:
    values = []
    for item in items:
        created_at = item.get("created_at")
        expires_at = item.get("expires_at")
        if created_at is None or expires_at is None:
            continue
        value = (parse_time(expires_at) - parse_time(created_at)).total_seconds()
        if value < 0:
            raise PerformanceError("negative artifact/cache retention duration")
        values.append(value)
    if not values:
        return {
            "status": "unavailable",
            "reason": "provider did not expose created_at and expires_at",
        }
    return {"min": min(values), "max": max(values)}


def load_collection(path: Path, *keys: str) -> list[dict[str, Any]]:
    payload = json.loads(path.read_text())
    if isinstance(payload, list):
        values = payload
    elif isinstance(payload, dict):
        values = None
        for key in keys:
            candidate = payload.get(key)
            if isinstance(candidate, list):
                values = candidate
                break
        if values is None:
            raise PerformanceError(f"{path} does not contain a supported collection")
    else:
        raise PerformanceError(f"{path} must contain a JSON collection")
    if not all(isinstance(item, dict) for item in values):
        raise PerformanceError(f"{path} contains a non-object collection item")
    return values


def observability_metrics(
    jobs: list[dict[str, Any]],
    run_payload: dict[str, Any],
    artifacts: list[dict[str, Any]],
    caches: list[dict[str, Any]],
    cache_scope: str | None = None,
) -> dict[str, Any]:
    runner_total = sum(duration(job) for job in jobs)
    run_id = str(run_payload.get("id", ""))
    run_attempt = str(run_payload.get("run_attempt", "1"))
    if cache_scope is None:
        cache_scope = f"perf-{run_id}-{run_attempt}"
    matching_caches = [
        cache
        for cache in caches
        if cache_scope in str(cache.get("key", ""))
        or cache_scope in str(cache.get("version", ""))
    ]
    artifact_bytes = 0
    for artifact in artifacts:
        size = artifact.get("size_in_bytes", 0)
        if not isinstance(size, int) or size < 0:
            raise PerformanceError(f"invalid artifact size: {size!r}")
        artifact_bytes += size
    cache_bytes = 0
    cache_size_by_key: dict[str, int] = {}
    for cache in matching_caches:
        size = cache.get("size_in_bytes", 0)
        if not isinstance(size, int) or size < 0:
            raise PerformanceError(f"invalid cache size: {size!r}")
        cache_bytes += size
        key = str(cache.get("key", ""))
        cache_size_by_key[key] = cache_size_by_key.get(key, 0) + size
    return {
        "required_wall_clock_seconds": required_wall_clock_seconds(jobs),
        "runner_total_seconds": runner_total,
        "run_count": 1,
        "job_count": len(jobs),
        "artifact_count": len(artifacts),
        "artifact_size_bytes": artifact_bytes,
        "artifact_retention_seconds": retention_seconds(artifacts),
        "cache": {
            "scope": cache_scope,
            "matching_entry_count": len(matching_caches),
            "size_bytes": cache_bytes,
            "size_by_key": cache_size_by_key,
            "retention_seconds": retention_seconds(matching_caches),
        },
    }


def assert_ci(jobs: list[dict[str, Any]], receipts: list[dict[str, Any]]) -> dict[str, Any]:
    components = ("rust", "rust-clippy", "rust-contracts", "docs", "web", "storybook", "docker-smoke")
    cache_components = ("rust", "rust-clippy", "rust-contracts", "web", "storybook", "docker-smoke")
    samples: dict[str, Any] = {}
    for sample in range(1, 4):
        phase_seconds: dict[str, float] = {}
        for phase, limit in CI_PHASE_LIMITS:
            durations = {}
            for component in components:
                job = find_one(
                    jobs,
                    rf"sample-{sample} / {phase} / {re.escape(component)}",
                )
                durations[component] = duration(job)
            critical = max(durations.values())
            if critical > limit:
                raise PerformanceError(
                    f"{phase}-{sample} critical path {critical:.1f}s exceeds {limit}s"
                )
            phase_seconds[phase] = critical

            for component in cache_components:
                matches = [
                    receipt
                    for receipt in receipts
                    if receipt.get("sample") == str(sample)
                    and receipt.get("phase") == phase
                    and receipt.get("component") == component
                ]
                if len(matches) != 1:
                    raise PerformanceError(
                        f"expected one cache receipt for {phase}-{sample}/{component}"
                    )
                expected = phase == "warm"
                if matches[0].get("cache_hit") is not expected:
                    raise PerformanceError(
                        f"cache receipt mismatch for {phase}-{sample}/{component}"
                    )
        samples[str(sample)] = phase_seconds
    verify = find_optional(jobs, r"verify")
    assertion = find_optional(jobs, r"assert")
    critical = sum(
        duration(job)
        for job in (verify, assertion)
        if job is not None
    ) + sum(
        samples[str(sample)]["cold"] + samples[str(sample)]["warm"]
        for sample in range(1, 4)
    )
    return {
        "kind": "ci",
        "samples": samples,
        "critical_path_seconds": critical,
        "status": "pass",
    }


def assert_release(jobs: list[dict[str, Any]]) -> dict[str, Any]:
    seconds = {
        "web": duration(find_one(jobs, r"build / build-web")),
        "musl-x86_64": duration(find_one(jobs, r"build / build-musl \(x86_64.*\)")),
        "musl-aarch64": duration(find_one(jobs, r"build / build-musl \(aarch64.*\)")),
        "runtime-x86_64": duration(find_one(jobs, r"build / build-runtime \(x86_64.*\)")),
        "runtime-aarch64": duration(find_one(jobs, r"build / build-runtime \(aarch64.*\)")),
        "assemble": duration(find_one(jobs, r"assemble")),
    }
    limits = {
        "web": 120,
        "musl-x86_64": 600,
        "musl-aarch64": 600,
        "runtime-x86_64": 180,
        "runtime-aarch64": 180,
        "assemble": 300,
    }
    for name, limit in limits.items():
        if seconds[name] > limit:
            raise PerformanceError(f"{name} {seconds[name]:.1f}s exceeds {limit}s")
    web_musl = seconds["web"] + max(seconds["musl-x86_64"], seconds["musl-aarch64"])
    runtime = max(seconds["runtime-x86_64"], seconds["runtime-aarch64"])
    verify = find_optional(jobs, r"verify")
    assertion = find_optional(jobs, r"assert")
    critical = (
        sum(
            duration(job)
            for job in (verify, assertion)
            if job is not None
        )
        + max(web_musl, runtime)
        + seconds["assemble"]
    )
    if critical > 900:
        raise PerformanceError(f"release critical path {critical:.1f}s exceeds 900s")
    return {"kind": "release", "seconds": seconds, "critical_path_seconds": critical, "status": "pass"}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--kind", choices=("ci", "release"), required=True)
    parser.add_argument("--jobs", type=Path, required=True)
    parser.add_argument("--run", type=Path, required=True)
    parser.add_argument("--receipts", type=Path)
    parser.add_argument("--artifacts", type=Path, required=True)
    parser.add_argument("--caches", type=Path, required=True)
    parser.add_argument("--cache-scope")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    payload = json.loads(args.jobs.read_text())
    jobs = payload.get("jobs") if isinstance(payload, dict) else None
    if not isinstance(jobs, list):
        raise PerformanceError("jobs payload must contain a jobs array")
    run_payload = json.loads(args.run.read_text())
    if not isinstance(run_payload, dict):
        raise PerformanceError("run payload must be an object")
    if args.kind == "ci":
        if args.receipts is None:
            raise PerformanceError("CI assertions require cache receipts")
        result = assert_ci(jobs, load_receipts(args.receipts))
    else:
        result = assert_release(jobs)
    result.update(
        observability_metrics(
            jobs,
            run_payload,
            load_collection(args.artifacts, "artifacts"),
            load_collection(args.caches, "caches", "actions_caches"),
            args.cache_scope,
        )
    )
    result["workflow_queue_seconds"] = workflow_queue_seconds(run_payload)
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
