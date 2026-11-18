"""Regression coverage for bounded managed-ingress ownership observations."""

import importlib.util
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "native_reality_host", Path(__file__).with_name("native-reality-host.py")
)
PROBE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PROBE)

BARE = "LISTEN 0 4096 *:26443 *:*\n"
OWNED = 'LISTEN 0 4096 *:26443 *:* users:(("xray",pid=42,fd=7))\n'


class OwnershipObservationTests(unittest.TestCase):
    def observe(self, rows, *, accepted, response="204 2"):
        samples = iter(rows)
        with (
            patch.object(
                PROBE, "run",
                side_effect=lambda *args, **kwargs: subprocess.CompletedProcess(
                    args, 0, next(samples), ""
                ),
            ) as listeners,
            patch.object(
                PROBE.subprocess, "run",
                return_value=subprocess.CompletedProcess([], 0, response, ""),
            ) as https,
            patch.object(PROBE.time, "sleep"),
        ):
            if accepted:
                PROBE.assert_ingress("xray")
            else:
                with self.assertRaises(AssertionError):
                    PROBE.assert_ingress("xray")
            return listeners.call_count, https.call_count

    def test_missing_owner_then_confirmed_owner(self):
        self.assertEqual(self.observe([BARE, OWNED], accepted=True), (2, 1))

    def test_permanent_missing_owner_exhausts_bound_without_https(self):
        self.assertEqual(self.observe([BARE] * 80, accepted=False), (80, 0))

    def test_wrong_known_owner_fails_immediately(self):
        self.assertEqual(
            self.observe([OWNED.replace("xray", "other")], accepted=False), (1, 0)
        )

    def test_multiple_listeners_fail_immediately(self):
        self.assertEqual(self.observe([OWNED + OWNED], accepted=False), (1, 0))

    def test_absent_listener_then_confirmed_owner(self):
        self.assertEqual(self.observe(["", OWNED], accepted=True), (2, 2))

    def test_https_without_http2_exhausts_bound(self):
        self.assertEqual(
            self.observe([OWNED] * 80, accepted=False, response="204 1.1"), (80, 80)
        )


if __name__ == "__main__":
    unittest.main()
