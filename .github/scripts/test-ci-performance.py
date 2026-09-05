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
    def valid_jobs(self):
        components = ("rust", "rust-contracts", "docs", "web", "storybook", "docker-smoke")
        return [job(f"{phase}-{sample} / {component}") for sample in range(1, 4) for phase in ("cold", "warm") for component in components]

    def valid_receipts(self):
        components = ("rust", "rust-contracts", "web", "storybook", "docker-smoke")
        return [{"sample": str(sample), "phase": phase, "component": component, "cache_hit": phase == "warm"} for sample in range(1, 4) for phase in ("cold", "warm") for component in components]

    def test_ci_accepts_three_exact_cold_warm_pairs(self):
        result = performance.assert_ci(self.valid_jobs(), self.valid_receipts())
        self.assertEqual(result["status"], "pass")

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
        jobs[0] = job(jobs[0]["name"], 421)
        with self.assertRaises(performance.PerformanceError):
            performance.assert_ci(jobs, self.valid_receipts())

    def test_workflow_queue_is_reported_separately(self):
        payload = {
            "created_at": "2026-09-05T00:00:00Z",
            "run_started_at": "2026-09-05T00:00:07Z",
        }
        self.assertEqual(performance.workflow_queue_seconds(payload), 7)


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


class WorkflowContractTests(unittest.TestCase):
    def test_performance_workflows_are_manual_and_read_only(self):
        workflows = Path(__file__).parents[1] / "workflows"
        for name in ("ci-performance.yml", "release-performance.yml"):
            text = (workflows / name).read_text()
            self.assertIn("workflow_dispatch:", text)
            self.assertIn("contents: read", text)
            self.assertNotIn("contents: write", text)
            self.assertNotIn("packages: write", text)

    def test_ci_declares_three_cold_warm_samples_and_hard_limits(self):
        workflows = Path(__file__).parents[1] / "workflows"
        text = (workflows / "ci-performance.yml").read_text()
        self.assertIn("sample: [1, 2, 3]", text)
        self.assertIn("cache_phase: cold", text)
        self.assertIn("cache_phase: warm", text)
        assertions = (SCRIPT_DIR / "performance_assert.py").read_text()
        self.assertIn('(\"cold\", 420)', assertions)
        self.assertIn('(\"warm\", 300)', assertions)

    def test_release_and_measurement_share_the_build_workflow(self):
        workflows = Path(__file__).parents[1] / "workflows"
        release = (workflows / "release.yml").read_text()
        measurement = (workflows / "release-performance.yml").read_text()
        self.assertIn("uses: ./.github/workflows/release-build.yml", release)
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
