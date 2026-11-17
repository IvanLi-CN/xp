import copy
from datetime import datetime, timezone, timedelta
from pathlib import Path
import json
import tempfile
import unittest
from policy import related, gate, validate_receipt, disposition
from benchmark import compare, verify, digest, CONFIG, ROOT
from github import seal


class PolicyTests(unittest.TestCase):
    def test_consumer_rejects_stale_or_incomplete_binary_manifest(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            for name in ("xp-proxy", "perf_harness"):
                (directory / name).write_bytes(b"fixture binary")
            valid = dict(
                schema_version=1, sha="a" * 40, target=CONFIG["target"],
                profile=CONFIG["profile"], toolchain=CONFIG["toolchain"],
                config_digest=digest(ROOT / "config.json"),
                checksums={name: digest(directory / name) for name in ("xp-proxy", "perf_harness")},
            )
            manifest = directory / "manifest.json"
            manifest.write_text(json.dumps(valid))
            verify(directory, "a" * 40, "candidate")
            for key in ("sha", "target", "profile", "toolchain", "config_digest"):
                invalid = {**valid, key: "wrong"}
                manifest.write_text(json.dumps(invalid))
                with self.assertRaises(ValueError, msg=key):
                    verify(directory, "a" * 40, "candidate")
            invalid = {**valid, "checksums": {"xp-proxy": valid["checksums"]["xp-proxy"]}}
            manifest.write_text(json.dumps(invalid))
            with self.assertRaises(ValueError):
                verify(directory, "a" * 40, "candidate")
            manifest.write_text(json.dumps(valid))
            (directory / "xp-proxy").write_bytes(b"corrupt")
            with self.assertRaises(ValueError):
                verify(directory, "a" * 40, "candidate")

    def test_related_inputs_and_not_applicable(self):
        for path in (
            "crates/xp-proxy/src/main.rs",
            "crates/xp-proxy-core/src/xhttp.rs",
            "Cargo.lock",
            "Cargo.toml",
            ".cargo/config.toml",
            "rust-toolchain.toml",
            "scripts/perf/xp-proxy/config.json",
            ".github/workflows/xp-proxy-perf-build.yml",
        ):
            self.assertTrue(related(path), path)
        for path in ("web/src/App.tsx", "docs/ops/README.md", "src/raft.rs"):
            self.assertFalse(related(path), path)

    def test_waiver_changes_only_gate(self):
        for measurement in ("success", "failure", "error", "cancelled", "pending"):
            with self.subTest(measurement=measurement):
                kwargs = dict(
                    applicable=True,
                    waived=True,
                    measurement=measurement,
                    current_sha="a",
                    measured_sha="a",
                )
                self.assertEqual(gate(**kwargs)[0], "success")
                kwargs["waived"] = False
                self.assertEqual(
                    gate(**kwargs)[0],
                    "success"
                    if measurement == "success"
                    else "pending"
                    if measurement == "pending"
                    else "failure",
                )
        self.assertEqual(
            gate(
                applicable=True,
                waived=False,
                measurement="success",
                current_sha="b",
                measured_sha="a",
            )[0],
            "pending",
        )
        self.assertEqual(
            gate(
                applicable=False,
                waived=False,
                measurement="missing",
                current_sha="a",
                measured_sha="a",
            )[0],
            "success",
        )

    def trusted(self):
        run = dict(
            repository=dict(id=1),
            workflow_id=2,
            event="workflow_dispatch",
            head_branch="main",
            id=3,
            run_attempt=1,
            head_sha="a" * 40,
            path=".github/workflows/xp-proxy-perf-result.yml",
            triggering_actor=dict(login="owner"),
        )
        receipt = dict(
            repository_id=1,
            operation_run_id=3,
            operation_run_attempt=1,
            workflow_sha="a" * 40,
            workflow_path=run["path"],
            workflow_ref="refs/heads/main",
            actor="owner",
            reason="noisy host",
            created_at=datetime.now(timezone.utc).isoformat(),
            action="invalidate",
        )
        return receipt, run

    def test_receipt_requires_actual_workflow_origin(self):
        receipt, run = self.trusted()
        validate_receipt(
            receipt, run, repository_id=1, workflow_id=2, default_branch="main"
        )
        for key, value in (
            ("workflow_id", 7),
            ("event", "pull_request"),
            ("head_branch", "topic"),
            ("head_sha", "b" * 40),
            ("run_attempt", 2),
            ("repository", dict(id=9)),
            ("triggering_actor", dict(login="someone")),
        ):
            fake = copy.deepcopy(run)
            fake[key] = value
            with self.assertRaises(ValueError, msg=key):
                validate_receipt(
                    receipt, fake, repository_id=1, workflow_id=2, default_branch="main"
                )
        receipt["created_at"] = (
            datetime.now(timezone.utc) - timedelta(days=91)
        ).isoformat()
        with self.assertRaises(ValueError):
            validate_receipt(
                receipt, run, repository_id=1, workflow_id=2, default_branch="main"
            )

    def test_disposition_is_append_only_attempt_scoped(self):
        identity = [1, 5, 1]
        first = dict(
            target_identity=identity,
            sequence=1,
            previous_digest=None,
            action="invalidate",
            operation_run_id=9,
        )
        first["digest"] = seal(first)
        second = dict(
            target_identity=identity,
            sequence=2,
            previous_digest=first["digest"],
            action="restore",
            operation_run_id=10,
        )
        second["digest"] = seal(second)
        self.assertEqual(disposition([first], identity), "invalidated")
        self.assertEqual(disposition([second, first], identity), "valid")
        self.assertEqual(disposition([first], [1, 5, 2]), "valid")
        with self.assertRaises(ValueError):
            disposition([second], identity)
        with self.assertRaises(ValueError):
            disposition([first, first], identity)
        second["reason"] = "changed"
        self.assertNotEqual(second["digest"], seal(second))

    def test_fixed_thresholds_and_no_missing_samples_success(self):
        rows = [
            dict(
                program=name,
                direction="inbound",
                concurrency=1,
                workload=workload,
                sweep=sweep,
                samples=100,
                confirmed_bytes=1024 * 1024,
                mbps=100,
                p50_ms=1,
                p95_ms=2,
                errors=[],
                resources_released=True,
            )
            for name in ("candidate", "main", "Xray")
            for workload in ("latency", "upload", "download")
            for sweep in range(2)
        ]
        self.assertEqual(compare(rows), [])
        rows[2]["mbps"] = 89
        self.assertTrue(compare(rows))
        rows[2]["mbps"] = 100
        rows[0]["samples"] = 99
        self.assertTrue(compare(rows))
        rows[0]["samples"] = 100
        rows[0]["errors"] = ["bad checksum"]
        self.assertTrue(compare(rows))
        self.assertTrue(compare(rows[:-1]))

    def test_workflows_separate_measurement_and_mark_privileges(self):
        root = Path(__file__).resolve().parents[3]
        workflow = (root / ".github/workflows/xp-proxy-perf.yml").read_text()
        measurement = workflow.split("  measurement:")[1].split("  gate:")[0]
        self.assertNotIn("cargo ", measurement)
        self.assertNotIn("perf:waive", measurement)
        self.assertNotIn("checks: write", measurement)
        self.assertIn("needs: [request, candidate, baseline]", measurement)
        mark = (root / ".github/workflows/xp-proxy-perf-result.yml").read_text()
        self.assertIn("ref: '${{ github.workflow_sha }}'", mark)
        self.assertLess(
            mark.index("actions/upload-artifact"), mark.index("checks: write")
        )
        self.assertIn("cancel-in-progress: false", mark)


if __name__ == "__main__":
    unittest.main()
