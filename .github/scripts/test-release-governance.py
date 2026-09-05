#!/usr/bin/env python3
"""Deterministic tests for release intent and readiness contracts."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import sys
import unittest

sys.dont_write_bytecode = True


SCRIPT_DIR = Path(__file__).parent


def load(name: str):
    spec = importlib.util.spec_from_file_location(name, SCRIPT_DIR / f"{name}.py")
    if spec is None or spec.loader is None:
        raise RuntimeError(name)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


intent = load("release_intent")
readiness = load("release_readiness")


class ReleaseIntentTests(unittest.TestCase):
    def test_pagination_requires_link_for_full_page(self) -> None:
        class FakeApi(intent.GitHubApi):
            def __init__(self, responses):
                super().__init__("https://example.invalid", "token")
                self.responses = iter(responses)

            def get_json_with_headers(self, path):
                return next(self.responses)

        full_page = [{"id": number} for number in range(100)]
        with self.assertRaises(intent.IntentError):
            FakeApi([(full_page, {})]).get_pages("/events")

        paged = FakeApi(
            [
                (full_page, {"link": '<https://example.invalid/events?page=2>; rel="next"'}),
                ([{"id": 100}], {"link": '<https://example.invalid/events?page=2>; rel="last"'}),
            ]
        )
        self.assertEqual(len(paged.get_pages("/events")), 101)

    def test_labels_are_replayed_only_until_merge_event(self) -> None:
        events = [
            {"id": 1, "event": "labeled", "label": {"name": "type:minor"}},
            {"id": 2, "event": "labeled", "label": {"name": "channel:stable"}},
            {"id": 3, "event": "merged"},
            {"id": 4, "event": "unlabeled", "label": {"name": "type:minor"}},
            {"id": 5, "event": "labeled", "label": {"name": "type:major"}},
        ]
        labels = intent.labels_at_merge(events, "a" * 40)
        self.assertEqual(labels, {"type:minor", "channel:stable"})
        self.assertEqual(intent.validate_labels(labels)[:3], ("minor", "stable", True))

    def test_closed_merge_event_is_supported_when_merged_event_is_absent(self) -> None:
        events = [
            {"id": 1, "event": "labeled", "label": {"name": "type:patch"}},
            {"id": 2, "event": "labeled", "label": {"name": "channel:prerelease"}},
            {"id": 3, "event": "closed", "commit_id": "a" * 40},
        ]
        self.assertEqual(intent.labels_at_merge(events, "a" * 40), {"type:patch", "channel:prerelease"})

    def test_event_history_does_not_require_pull_request_open_event(self) -> None:
        pull_request = intent.PullRequest(
            number=1,
            url="",
            merge_commit_sha="a" * 40,
            created_at="2026-08-01T00:00:00Z",
            merged_at="2026-08-02T00:00:00Z",
            base_ref="main",
        )
        intent.verify_event_history(
            [{"id": 1, "event": "merged", "created_at": "2026-08-02T00:00:00Z"}],
            pull_request,
        )

    def test_event_history_requires_timestamps_and_merge_event(self) -> None:
        pull_request = intent.PullRequest(
            number=1,
            url="",
            merge_commit_sha="a" * 40,
            created_at="2026-08-01T00:00:00Z",
            merged_at="2026-08-02T00:00:00Z",
            base_ref="main",
        )
        with self.assertRaises(intent.IntentError):
            intent.verify_event_history([{"id": 1, "event": "labeled"}], pull_request)
        with self.assertRaises(intent.IntentError):
            intent.verify_event_history(
                [{"id": 1, "event": "closed", "created_at": "2026-08-02T00:00:00Z"}],
                pull_request,
            )

    def test_duplicate_and_unknown_labels_fail_closed(self) -> None:
        with self.assertRaises(intent.IntentError):
            intent.validate_labels({"type:minor", "type:patch", "channel:stable"})
        with self.assertRaises(intent.IntentError):
            intent.validate_labels({"type:minor", "channel:unknown"})

    def test_manual_version_and_reason_are_strict(self) -> None:
        intent.validate_manual_inputs("minor", "stable", "3.33.0", "owner approved correction")
        with self.assertRaises(intent.IntentError):
            intent.validate_manual_inputs("minor", "stable", "3.33.1", "bad\nreason")
        with self.assertRaises(intent.IntentError):
            intent.validate_manual_inputs("minor", "stable", "3.33.0-rc.1", "bad channel")


class ReadinessTests(unittest.TestCase):
    def test_exact_sha_success_is_ready(self) -> None:
        sha = "b" * 40
        payload = {"workflow_runs": [
            {"id": 1, "name": "ci", "head_sha": sha, "event": "push", "head_branch": "main", "status": "completed", "conclusion": "success"},
            {"id": 2, "name": "fixture-policy", "head_sha": sha, "event": "push", "head_branch": "main", "status": "completed", "conclusion": "success"},
            {"id": 3, "name": "xray-e2e", "head_sha": sha, "event": "push", "head_branch": "main", "status": "completed", "conclusion": "success"},
        ]}
        self.assertEqual(readiness.evaluate(payload, sha), ("ready", "all_required_workflows_success"))

    def test_missing_and_failed_runs_are_distinguished(self) -> None:
        sha = "c" * 40
        payload = {"workflow_runs": [
            {"id": 1, "name": "ci", "head_sha": sha, "event": "push", "status": "completed", "conclusion": "success"},
            {"id": 2, "name": "fixture-policy", "head_sha": sha, "event": "push", "head_branch": "main", "status": "completed", "conclusion": "failure"},
        ]}
        status, detail = readiness.evaluate(payload, sha)
        self.assertEqual(status, "failed")
        self.assertIn("fixture-policy:completed/failure:2", detail)

        pending, pending_detail = readiness.evaluate({"workflow_runs": []}, sha)
        self.assertEqual(pending, "pending")
        self.assertIn("xray-e2e", pending_detail)

    def test_wrong_sha_or_event_does_not_satisfy_gate(self) -> None:
        sha = "d" * 40
        payload = {"workflow_runs": [
            {"id": 1, "name": "ci", "head_sha": sha, "event": "pull_request", "status": "completed", "conclusion": "success"},
        ]}
        status, _ = readiness.evaluate(payload, sha)
        self.assertEqual(status, "pending")

    def test_missing_main_branch_is_not_accepted(self) -> None:
        sha = "e" * 40
        payload = {
            "workflow_runs": [
                {"id": 1, "name": name, "head_sha": sha, "event": "push",
                 "status": "completed", "conclusion": "success"}
                for name in readiness.REQUIRED_WORKFLOWS
            ]
        }
        status, detail = readiness.evaluate(payload, sha)
        self.assertEqual(status, "pending")
        self.assertIn("ci:missing", detail)

    def test_readiness_diagnostics_include_every_workflow(self) -> None:
        sha = "f" * 40
        payload = {
            "workflow_runs": [
                {"id": 1, "name": "ci", "head_sha": sha, "event": "push",
                 "head_branch": "main", "status": "completed", "conclusion": "failure"},
            ]
        }
        status, detail = readiness.evaluate(payload, sha)
        self.assertEqual(status, "failed")
        self.assertIn("ci:completed/failure:1", detail)
        self.assertIn("fixture-policy:missing", detail)
        self.assertIn("xray-e2e:missing", detail)


class WorkflowIntegrationTests(unittest.TestCase):
    def test_release_workflow_exposes_backfill_contract(self) -> None:
        workflow = (Path(__file__).parents[1] / "workflows" / "release.yml").read_text()
        for input_name in ("head_sha", "release_type", "channel", "expected_version", "reason"):
            self.assertIn(f"      {input_name}:", workflow)
        self.assertIn("release_readiness.py", workflow)
        self.assertIn("verify expected backfill version", workflow)
        self.assertIn("uses: ./.github/workflows/release-build.yml", workflow)
        self.assertNotIn("cargo clippy", workflow)
        self.assertNotIn("cargo test", workflow)

    def test_label_gate_requires_unique_type_and_channel(self) -> None:
        workflow = (Path(__file__).parents[1] / "workflows" / "label-gate.yml").read_text()
        self.assertIn("listLabelsOnIssue", workflow)
        self.assertIn("exactly one release type label", workflow)
        self.assertIn("exactly one release channel label", workflow)


if __name__ == "__main__":
    unittest.main()
