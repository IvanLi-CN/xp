#!/usr/bin/env python3
"""Fail-closed performance assertions for CI and release workflow jobs."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import re
from typing import Any


class PerformanceError(RuntimeError):
    pass


CI_PHASE_LIMITS = (("cold", 540), ("warm", 300))

CI_WORKLOAD_STEPS = {
    "rust": (
        "build embedded web assets",
        "Run cargo test",
        "test embedded web asset routes",
    ),
    "rust-clippy": ("build embedded web assets", "Run cargo clippy -- -D warnings"),
    "rust-contracts": (
        "repository contract tests",
        "Cargo source marker cache contract",
        "contract xp-ops builds without web/dist",
        "xp-ops dry-run contracts",
    ),
    "docs": ("Run bunx --no-install dprint check",),
    "web": (
        "Run bun run lint",
        "Run bun run typecheck",
        "Run bun run test",
        "Run bunx --no-install playwright install --with-deps chromium",
        "Run bun run test:e2e",
    ),
    "storybook": (
        "Run bunx --no-install playwright install --with-deps chromium",
        "storybook and test-storybook",
    ),
    "docker-smoke": ("docker build smoke", "default entrypoint bootstrap dry-run"),
}


def parse_time(value: Any) -> datetime:
    if not isinstance(value, str) or not value.endswith("Z"):
        raise PerformanceError(f"invalid workflow timestamp: {value!r}")
    try:
        return datetime.fromisoformat(value.removesuffix("Z") + "+00:00")
    except ValueError as error:
        raise PerformanceError(f"invalid workflow timestamp: {value!r}") from error


def duration(job: dict[str, Any]) -> float:
    if job.get("conclusion") != "success":
        raise PerformanceError(
            f"job did not succeed: {job.get('name')}={job.get('conclusion')}"
        )
    elapsed = (parse_time(job.get("completed_at")) - parse_time(job.get("started_at"))).total_seconds()
    if elapsed < 0:
        raise PerformanceError(f"negative duration: {job.get('name')}")
    return elapsed


def step_duration(step: dict[str, Any]) -> float:
    if step.get("conclusion") != "success":
        raise PerformanceError(f"workload step did not succeed: {step.get('name')}")
    elapsed = (parse_time(step.get("completed_at")) - parse_time(step.get("started_at"))).total_seconds()
    if elapsed < 0:
        raise PerformanceError(f"negative workload step duration: {step.get('name')}")
    return elapsed


def ci_workload_duration(job: dict[str, Any]) -> float:
    """Measure declared workload steps while retaining full job metrics separately."""
    # Validate the complete job before measuring only the declared workload steps.
    # A successful workload must not hide a failed setup, cleanup, or unlisted step.
    duration(job)
    name = str(job.get("name", ""))
    match = re.fullmatch(r"sample-[1-3] / (?:cold|warm) / (.+)", name)
    if match is None:
        return duration(job)
    component = match.group(1)
    expected = CI_WORKLOAD_STEPS.get(component)
    steps = job.get("steps")
    if expected is None:
        return duration(job)
    if not isinstance(steps, list):
        raise PerformanceError(f"missing workload steps for {name}")
    by_name = {str(step.get("name")): step for step in steps if isinstance(step, dict)}
    missing = [step_name for step_name in expected if step_name not in by_name]
    if missing:
        raise PerformanceError(f"missing workload steps for {name}: {missing}")
    return sum(step_duration(by_name[step_name]) for step_name in expected)


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


def materialize_current_job(
    jobs: list[dict[str, Any]],
    name: str = "assert",
    observed_at: str | None = None,
) -> list[dict[str, Any]]:
    if not all(isinstance(job, dict) for job in jobs):
        raise PerformanceError("jobs payload contains a non-object job")
    matches = [job for job in jobs if job.get("name") == name]
    if len(matches) != 1:
        raise PerformanceError(f"expected one current assertion job {name!r}, got {len(matches)}")
    current = matches[0]
    if current.get("conclusion") == "success" and current.get("completed_at"):
        return jobs
    if current.get("conclusion") not in (None, ""):
        return jobs
    parse_time(current.get("started_at"))
    if observed_at is None:
        observed_at = datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace(
            "+00:00", "Z"
        )
    parse_time(observed_at)
    snapshot = dict(current)
    snapshot["conclusion"] = "success"
    snapshot["completed_at"] = observed_at
    return [snapshot if job is current else job for job in jobs]


def retention_seconds(items: list[dict[str, Any]]) -> dict[str, Any]:
    values = []
    for item in items:
        created_at = item.get("created_at")
        expires_at = item.get("expires_at")
        if created_at is None or expires_at is None:
            return {
                "status": "unavailable",
                "reason": "provider did not expose created_at and expires_at",
            }
        value = (parse_time(expires_at) - parse_time(created_at)).total_seconds()
        if value < 0:
            raise PerformanceError("negative artifact/cache retention duration")
        values.append(value)
    if not values:
        return {
            "status": "unavailable",
            "reason": "provider did not expose created_at and expires_at",
        }
    return {"status": "available", "min_seconds": min(values), "max_seconds": max(values)}


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
        if f"-{cache_scope}-" in f"-{str(cache.get('key', ''))}-"
    ]
    if not artifacts:
        raise PerformanceError("required artifact evidence is missing")
    if not matching_caches:
        raise PerformanceError(f"required cache evidence is missing for {cache_scope}")
    artifact_bytes = 0
    for artifact in artifacts:
        size = artifact.get("size_in_bytes")
        if isinstance(size, bool) or not isinstance(size, int) or size < 0:
            raise PerformanceError(f"invalid artifact size: {size!r}")
        artifact_bytes += size
    cache_bytes = 0
    cache_size_by_key: dict[str, int] = {}
    for cache in matching_caches:
        size = cache.get("size_in_bytes")
        if isinstance(size, bool) or not isinstance(size, int) or size < 0:
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
    cache_components = (
        "rust", "rust-clippy", "rust-contracts", "docs", "web", "storybook", "docker-smoke"
    )
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
                # The hard gate follows the repository contract's declared
                # workload steps. Complete job wall-clock remains reported
                # separately in observability metrics.
                durations[component] = ci_workload_duration(job)
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
    verify = find_one(jobs, r"verify")
    assertion = find_one(jobs, r"assert")
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


RELEASE_CACHE_PREFIXES = (
    "release-web-",
    "release-musl-x86_64-unknown-linux-musl-",
    "release-musl-aarch64-unknown-linux-musl-",
    "release-go-amd64-",
    "release-go-arm64-",
)


def assert_release(
    jobs: list[dict[str, Any]],
    caches: list[dict[str, Any]] | None = None,
    cache_scope: str | None = None,
) -> dict[str, Any]:
    if caches is not None or cache_scope is not None:
        if not caches or not cache_scope:
            raise PerformanceError("release cache evidence is incomplete")
        scoped_keys = {
            str(cache.get("key"))
            for cache in caches
            if f"-{cache_scope}-" in f"-{str(cache.get('key', ''))}-"
        }
        missing = [
            prefix
            for prefix in RELEASE_CACHE_PREFIXES
            if not any(f"-{prefix}" in key for key in scoped_keys)
        ]
        if missing:
            raise PerformanceError(
                f"release cache evidence is missing required scopes: {missing}"
            )
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
    verify = find_one(jobs, r"verify")
    assertion = find_one(jobs, r"assert")
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
    return {
        "kind": "release",
        "seconds": seconds,
        "critical_path_seconds": critical,
        "status": "pass",
        "buildkit_cache": {
            "status": "unavailable",
            "reason": "GitHub Actions cache API does not expose type=gha BuildKit attribution",
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--kind", choices=("ci", "release"), required=True)
    parser.add_argument("--target-sha", required=True)
    parser.add_argument("--jobs", type=Path, required=True)
    parser.add_argument("--run", type=Path, required=True)
    parser.add_argument("--receipts", type=Path)
    parser.add_argument("--artifacts", type=Path, required=True)
    parser.add_argument("--caches", type=Path, required=True)
    parser.add_argument("--cache-scope")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    try:
        payload = json.loads(args.jobs.read_text())
        jobs = payload.get("jobs") if isinstance(payload, dict) else None
        if not isinstance(jobs, list):
            raise PerformanceError("jobs payload must contain a jobs array")
        if not all(isinstance(job, dict) for job in jobs):
            raise PerformanceError("jobs payload contains a non-object job")
        jobs = materialize_current_job(jobs)
        run_payload = json.loads(args.run.read_text())
        if not isinstance(run_payload, dict):
            raise PerformanceError("run payload must be an object")
        if not re.fullmatch(r"[0-9a-f]{40}", args.target_sha):
            raise PerformanceError("target SHA must be a 40-character lowercase hexadecimal value")
        if run_payload.get("head_sha") != args.target_sha:
            raise PerformanceError("performance run SHA does not match target SHA")
    except (PerformanceError, json.JSONDecodeError, OSError) as error:
        failure = {"kind": args.kind, "status": "fail", "target_sha": args.target_sha, "error": str(error)}
        args.output.write_text(json.dumps(failure, indent=2, sort_keys=True) + "\n")
        print(json.dumps(failure, sort_keys=True))
        return 1
    try:
        artifacts = load_collection(args.artifacts, "artifacts")
        caches = load_collection(args.caches, "caches", "actions_caches")
        if args.kind == "ci":
            if args.receipts is None:
                raise PerformanceError("CI assertions require cache receipts")
            result = assert_ci(jobs, load_receipts(args.receipts))
        else:
            result = assert_release(jobs, caches, args.cache_scope)
        result.update(
            observability_metrics(
                jobs,
                run_payload,
                artifacts,
                caches,
                args.cache_scope,
            )
        )
        result["workflow_queue_seconds"] = workflow_queue_seconds(run_payload)
        result["target_sha"] = args.target_sha
        args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
        print(json.dumps(result, sort_keys=True))
        return 0
    except (PerformanceError, json.JSONDecodeError, OSError) as error:
        failure = {"kind": args.kind, "status": "fail", "target_sha": args.target_sha, "error": str(error)}
        args.output.write_text(json.dumps(failure, indent=2, sort_keys=True) + "\n")
        print(json.dumps(failure, sort_keys=True))
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
