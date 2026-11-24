import copy
from datetime import datetime, timezone, timedelta
from pathlib import Path
import json
import tempfile
import unittest
from policy import related, gate, validate_receipt, disposition
from benchmark import combine, compare, headroom, orders, verify, digest, CONFIG, ROOT
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
            "scripts/e2e/install-mihomo-v1.19.29.sh",
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

    def rows(self, rounds=3, mbps=100):
        return [
            dict(
                program=name,
                direction="inbound",
                concurrency=1,
                workload=workload,
                sweep=sweep,
                samples=100,
                confirmed_bytes=1024 * 1024,
                mbps=mbps,
                p50_ms=1,
                p95_ms=2,
                errors=[],
                resources_released=True,
            )
            for name in ("candidate", "main", "Xray")
            for workload in ("latency", "upload", "download")
            for sweep in range(rounds)
        ]

    def test_rotated_rounds_give_every_subject_every_position(self):
        positions = [list(order) for order in orders(3)]
        for name in ("candidate", "main", "Xray"):
            self.assertEqual(
                sorted(order.index(name) for order in positions), [0, 1, 2]
            )

    def test_fixed_thresholds_and_no_missing_samples_success(self):
        rows = self.rows()
        self.assertEqual(compare(rows, 3), [])
        # One depressed round is same-run noise, not a scenario regression.
        rows[3]["mbps"] = 80
        self.assertEqual(compare(rows, 3), [])
        # A majority of rounds below 90% is a scenario regression.
        rows[4]["mbps"] = 80
        self.assertTrue(compare(rows, 3))
        rows[4]["mbps"] = 100
        rows[0]["samples"] = 99
        self.assertTrue(compare(rows, 3))
        rows[0]["samples"] = 100
        rows[0]["errors"] = ["bad checksum"]
        self.assertTrue(compare(rows, 3))
        self.assertTrue(compare(rows[:-1], 3))

    def test_repeated_cell_keeps_worst_repeat_validity(self):
        combined = combine(
            [
                dict(mbps=100, p50_ms=1, p95_ms=2, samples=120,
                     confirmed_bytes=2 * 1024 * 1024, errors=[]),
                dict(mbps=90, p50_ms=1, p95_ms=2, samples=0,
                     confirmed_bytes=0, errors=["short read"]),
                dict(mbps=110, p50_ms=1, p95_ms=2, samples=130,
                     confirmed_bytes=2 * 1024 * 1024, errors=[]),
            ]
        )
        self.assertEqual(combined["mbps"], 100)
        self.assertEqual(combined["samples"], 0)
        self.assertEqual(combined["confirmed_bytes"], 0)
        self.assertEqual(combined["errors"], ["short read"])
        self.assertEqual(combined["repeat_mbps"], [100, 90, 110])

    def test_same_source_noise_sample_passes_the_gate(self):
        # 2026-11-24 push run 45399956338 measured the same commit d7871a83 as
        # both candidate and main. inbound/c8/upload was 2738.96 vs 3118.19 Mbps
        # (ratio 0.878) in one sweep and 3025.18 vs 3164.75 (0.956) in the other.
        rows = self.rows(rounds=3)
        observed = {
            ("candidate", 0): 2738.96,
            ("candidate", 1): 3025.18,
            ("candidate", 2): 3069.0,
            ("main", 0): 3118.19,
            ("main", 1): 3164.75,
            ("main", 2): 3093.0,
            ("Xray", 0): 2733.04,
            ("Xray", 1): 2697.92,
            ("Xray", 2): 2715.0,
        }
        for row in rows:
            if row["workload"] == "upload":
                row["mbps"] = observed[(row["program"], row["sweep"])]
        self.assertEqual(compare(rows, 3), [])
        # The same shape with a sustained 15% candidate regression must fail.
        for row in rows:
            if row["program"] == "candidate" and row["workload"] == "upload":
                row["mbps"] *= 0.85
        self.assertTrue(compare(rows, 3))

    def test_latency_uses_the_median_round_delta(self):
        rows = self.rows()
        rows[0]["p95_ms"] = 8
        self.assertEqual(compare(rows, 3), [])
        rows[1]["p95_ms"] = 8
        self.assertTrue(compare(rows, 3))
        rows[0]["p95_ms"] = None
        self.assertIn(
            "inbound/c1/latency: missing p95_ms sample vs main", compare(rows, 3)
        )

    def test_direct_control_needs_headroom_over_the_fastest_result(self):
        rows = self.rows()
        direct = [
            dict(
                workload=workload,
                samples=100,
                errors=[],
                mbps=130 if workload == "upload" else 1000,
            )
            for workload in ("latency", "upload", "download")
        ]
        self.assertEqual(headroom(rows, direct), [])
        direct[1]["mbps"] = 119
        self.assertEqual(
            headroom(rows, direct),
            ["upload: direct capacity has insufficient headroom"],
        )
        direct[1]["errors"] = ["short read"]
        self.assertIn(
            "upload: direct control missing or invalid", headroom(rows, direct)
        )
        self.assertIn(
            "upload: direct control missing or invalid", headroom(rows, direct[:2])
        )

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
