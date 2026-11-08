#!/usr/bin/env python3
"""Deterministic tests for workflow performance acceptance."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import re
import sys
import unittest

sys.dont_write_bytecode = True

SCRIPT_DIR = Path(__file__).parent
spec = importlib.util.spec_from_file_location("performance_assert", SCRIPT_DIR / "performance_assert.py")
if spec is None or spec.loader is None:
    raise RuntimeError("performance_assert")
performance = importlib.util.module_from_spec(spec)
sys.modules["performance_assert"] = performance
spec.loader.exec_module(performance)


def job(name: str, seconds: int = 60, conclusion: str = "success") -> dict[str, str]:
    return {
        "name": name,
        "started_at": "2026-09-05T00:00:00Z",
        "completed_at": f"2026-09-05T00:{seconds // 60:02d}:{seconds % 60:02d}Z",
        "conclusion": conclusion,
    }


class CiPerformanceTests(unittest.TestCase):
    def test_workflows_stream_paginated_jobs_into_jq(self):
        workflows = SCRIPT_DIR.parent / "workflows"
        for name in ("ci-performance.yml", "release-performance.yml"):
            workflow = (workflows / name).read_text()
            self.assertIn('gh api --paginate "${jobs_url}" | jq -s', workflow)
            self.assertIn('gh api --paginate "${artifacts_url}"', workflow)
            self.assertIn("jq -s '{artifacts:", workflow)
            self.assertIn('gh api --paginate "${caches_url}"', workflow)
            self.assertIn("jq -s '{caches:", workflow)
            self.assertNotIn("--jq -s", workflow)

    def test_performance_concurrency_only_cancels_pull_request_runs(self):
        workflows = SCRIPT_DIR.parent / "workflows"
        for name in ("ci-performance.yml", "release-performance.yml"):
            workflow = (workflows / name).read_text()
            self.assertIn("concurrency:", workflow)
            self.assertIn("cancel-in-progress: ${{ github.event_name == 'pull_request' }}", workflow)
            self.assertIn("format('manual-{0}',", workflow)
            self.assertIn("github.run_id)", workflow)

    def test_pr_performance_targets_the_head_sha_and_stabilizes_cargo_marker_time(self):
        workflows = SCRIPT_DIR.parent / "workflows"
        for name in ("ci-performance.yml", "release-performance.yml"):
            workflow = (workflows / name).read_text()
            self.assertIn("github.event.pull_request.head.sha || github.sha", workflow)
            self.assertIn(
                'if [[ "${GITHUB_EVENT_NAME}" = "pull_request" ]]',
                workflow,
            )
        ci = (workflows / "ci.yml").read_text()
        self.assertEqual(ci.count("normalize Cargo source marker timestamp"), 3)
        self.assertIn('git log -1 --format=%ct HEAD', ci)
        release_build = (workflows / "release-build.yml").read_text()
        resource = (workflows / "resource-evidence.yml").read_text()
        self.assertIn("normalize Cargo source marker timestamp", release_build)
        self.assertIn("normalize Cargo source marker timestamp", resource)

    def test_web_and_storybook_share_the_same_cache_key(self):
        workflow = (SCRIPT_DIR.parent / "workflows" / "ci.yml").read_text()
        self.assertGreaterEqual(
            workflow.count("${{ runner.os }}-web-${{ env.CACHE_NAMESPACE }}-${{"),
            2,
        )
        self.assertNotIn("${{ runner.os }}-storybook-${{ env.CACHE_NAMESPACE }}", workflow)

    def test_ci_samples_run_as_serial_cold_warm_pairs(self):
        workflows = SCRIPT_DIR.parent / "workflows"
        parent = (workflows / "ci-performance.yml").read_text()
        sample = (workflows / "ci-performance-sample.yml").read_text()
        self.assertIn("max-parallel: 1", parent)
        self.assertIn("needs: cold", sample)

    def valid_jobs(self):
        components = ("rust", "rust-clippy", "rust-contracts", "docs", "web", "storybook", "docker-smoke")
        return [job(f"sample-{sample} / {phase} / {component}") for sample in range(1, 4) for phase in ("cold", "warm") for component in components]

    def valid_receipts(self):
        components = ("rust", "rust-clippy", "rust-contracts", "web", "storybook", "docker-smoke")
        return [{"sample": str(sample), "phase": phase, "component": component, "cache_hit": phase == "warm"} for sample in range(1, 4) for phase in ("cold", "warm") for component in components]

    def test_ci_accepts_three_exact_cold_warm_pairs(self):
        result = performance.assert_ci(self.valid_jobs(), self.valid_receipts())
        self.assertEqual(result["status"], "pass")
        self.assertEqual(result["critical_path_seconds"], 360)

    def test_ci_fails_closed_for_missing_job_or_bad_cache_state(self):
        with self.assertRaises(performance.PerformanceError):
            performance.assert_ci(self.valid_jobs()[:-1], self.valid_receipts())
        receipts = self.valid_receipts()
        receipts[0]["cache_hit"] = True
        with self.assertRaises(performance.PerformanceError):
            performance.assert_ci(self.valid_jobs(), receipts)

    def test_ci_rejects_incomplete_timestamps_and_threshold_regressions(self):
        jobs = self.valid_jobs()
        jobs[0]["completed_at"] = None
        with self.assertRaises(performance.PerformanceError):
            performance.assert_ci(jobs, self.valid_receipts())
        jobs = self.valid_jobs()
        jobs[0] = job(jobs[0]["name"], 541)
        with self.assertRaises(performance.PerformanceError):
            performance.assert_ci(jobs, self.valid_receipts())
        jobs[0] = job(jobs[0]["name"], 540)
        self.assertEqual(performance.assert_ci(jobs, self.valid_receipts())["status"], "pass")

    def test_workflow_queue_is_reported_separately(self):
        payload = {
            "created_at": "2026-09-05T00:00:00Z",
            "run_started_at": "2026-09-05T00:00:07Z",
        }
        self.assertEqual(performance.workflow_queue_seconds(payload), 7)

    def test_observability_reports_job_artifact_and_cache_metrics(self):
        run = {
            "id": 123,
            "run_attempt": 1,
            "created_at": "2026-09-05T00:00:00Z",
            "run_started_at": "2026-09-05T00:00:07Z",
        }
        artifacts = [
            {
                "size_in_bytes": 7,
                "created_at": "2026-09-05T00:00:00Z",
                "expires_at": "2026-09-05T00:01:00Z",
            }
        ]
        caches = [
            {"key": "Linux-rust-perf-123-1-1", "size_in_bytes": 11},
            {"key": "Linux-rust-ci-1", "size_in_bytes": 99},
        ]
        result = performance.observability_metrics(
            self.valid_jobs(), run, artifacts, caches
        )
        self.assertEqual(result["run_count"], 1)
        self.assertEqual(result["job_count"], 42)
        self.assertEqual(result["artifact_size_bytes"], 7)
        self.assertEqual(result["cache"]["size_bytes"], 11)
        self.assertEqual(result["cache"]["size_by_key"]["Linux-rust-perf-123-1-1"], 11)
        self.assertEqual(result["artifact_retention_seconds"]["min"], 60)
        self.assertEqual(result["cache"]["retention_seconds"]["status"], "unavailable")

    def test_observability_materializes_the_running_assertion_job(self):
        run = {"id": 123, "run_attempt": 1}
        jobs = self.valid_jobs() + [job("assert")]
        jobs[-1]["completed_at"] = None
        jobs[-1]["conclusion"] = None
        effective = performance.materialize_current_job(
            jobs, observed_at="2026-09-05T01:00:00Z"
        )
        self.assertEqual(effective[-1]["conclusion"], "success")
        self.assertEqual(effective[-1]["completed_at"], "2026-09-05T01:00:00Z")
        result = performance.observability_metrics(effective, run, [], [])
        self.assertEqual(result["job_count"], 43)


class ReleasePerformanceTests(unittest.TestCase):
    def valid_jobs(self):
        return [
            job("build / build-web", 60),
            job("build / build-musl (x86_64, x86_64-unknown-linux-musl)", 500),
            job("build / build-musl (aarch64, aarch64-unknown-linux-musl)", 500),
            job("build / build-runtime (x86_64, amd64)", 120),
            job("build / build-runtime (aarch64, arm64)", 120),
            job("assemble", 240),
        ]

    def test_release_computes_declared_critical_path(self):
        result = performance.assert_release(self.valid_jobs())
        self.assertEqual(result["critical_path_seconds"], 800)

    def test_release_rejects_failed_or_slow_jobs(self):
        jobs = self.valid_jobs()
        jobs[1] = job(jobs[1]["name"], 601)
        with self.assertRaises(performance.PerformanceError):
            performance.assert_release(jobs)

    def test_release_critical_path_includes_runtime_branch(self):
        jobs = [
            job("build / build-web", 10),
            job("build / build-musl (x86_64, target)", 10),
            job("build / build-musl (aarch64, target)", 10),
            job("build / build-runtime (x86_64, amd64)", 120),
            job("build / build-runtime (aarch64, arm64)", 120),
            job("assemble", 10),
        ]
        result = performance.assert_release(jobs)
        self.assertEqual(result["critical_path_seconds"], 130)


class WorkflowContractTests(unittest.TestCase):
    def test_performance_workflows_are_manual_and_read_only(self):
        workflows = Path(__file__).parents[1] / "workflows"
        for name in ("ci-performance.yml", "release-performance.yml"):
            text = (workflows / name).read_text()
            self.assertIn("workflow_dispatch:", text)
            self.assertIn("pull_request:", text)
            self.assertIn("contents: read", text)
            self.assertNotIn("contents: write", text)
            self.assertNotIn("packages: write", text)

    def test_ci_declares_three_cold_warm_samples_and_hard_limits(self):
        workflows = Path(__file__).parents[1] / "workflows"
        text = (workflows / "ci-performance.yml").read_text()
        self.assertEqual(text.count('sample: ["1", "2", "3"]'), 1)
        self.assertIn("max-parallel: 1", text)
        sample = (workflows / "ci-performance-sample.yml").read_text()
        self.assertIn("cache_phase: cold", sample)
        self.assertIn("cache_phase: warm", sample)
        self.assertIn("github.run_attempt", sample)
        assertions = (SCRIPT_DIR / "performance_assert.py").read_text()
        self.assertIn('(\"cold\", 540)', assertions)
        self.assertIn('(\"warm\", 300)', assertions)

        ci = (workflows / "ci.yml").read_text()
        self.assertIn("path: .buildx-cache", ci)
        self.assertIn("cache-from:", ci)
        self.assertIn("type=local,src=.buildx-cache", ci)
        self.assertNotIn("docker-cache-marker", ci)

    def test_rust_cache_tracks_stable_web_fingerprint(self):
        root = Path(__file__).parents[2]
        build = (root / "build.rs").read_text()
        package = (root / "web/package.json").read_text()
        vite_config = (root / "web/vite.config.ts").read_text()
        workflow = (root / ".github/workflows/ci.yml").read_text()
        release_workflow = (root / ".github/workflows/release-build.yml").read_text()
        dockerfile = (root / "Dockerfile").read_text()
        self.assertIn("web/.xp-dist-fingerprint", build)
        self.assertNotIn('cargo:rerun-if-changed=web/dist', build)
        self.assertIn("update-web-dist-fingerprint.sh", package)
        self.assertIn('".xp-dist-fingerprint"', vite_config)
        self.assertIn("web/.xp-dist-fingerprint", workflow)
        self.assertIn('CARGO_BUILD_JOBS: "8"', workflow)
        self.assertIn('RUST_TEST_THREADS: "8"', workflow)
        self.assertIn("fingerprint embedded web assets", release_workflow)
        self.assertIn("COPY scripts/update-web-dist-fingerprint.sh", dockerfile)
        self.assertIn("COPY --from=web-builder /app/web/.xp-dist-fingerprint", dockerfile)

    def test_release_and_measurement_share_the_build_workflow(self):
        workflows = Path(__file__).parents[1] / "workflows"
        release = (workflows / "release.yml").read_text()
        measurement = (workflows / "release-performance.yml").read_text()
        self.assertIn("uses: ./.github/workflows/release-build.yml", release)
        self.assertIn("needs: [prepare, build, publish-static-web, publish-image]", release)
        self.assertIn("uses: ./.github/workflows/release-build.yml", measurement)
        self.assertIn("push: false", measurement)
        self.assertNotIn("ncipollo/release-action", measurement)
        self.assertNotIn("docker/login-action", measurement)

    def test_changed_workflows_pin_external_actions(self):
        workflows = Path(__file__).parents[1] / "workflows"
        for name in (
            "ci.yml",
            "ci-performance.yml",
            "release.yml",
            "release-build.yml",
            "release-performance.yml",
        ):
            text = (workflows / name).read_text()
            for reference in re.findall(r"uses:\s+([^\s]+)", text):
                if reference.startswith("./"):
                    continue
                self.assertRegex(reference, r"@[0-9a-f]{40}$", f"mutable action in {name}")


if __name__ == "__main__":
    unittest.main()
