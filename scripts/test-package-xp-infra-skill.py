#!/usr/bin/env python3
"""Exercise release packaging against a fresh isolated Git repository."""
import hashlib
import importlib.util
import json
from pathlib import Path
import os
import subprocess
import tarfile
import tempfile
import sys
sys.dont_write_bytecode = True
import unittest

SPEC = importlib.util.spec_from_file_location(
    "package_skill", Path(__file__).with_name("package-xp-infra-skill.py"),
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)
SOURCE = Path(__file__).resolve().parents[1] / "skills/xp-infra"


class PackageTest(unittest.TestCase):
    def test_committed_source_is_complete_reproducible_and_independent(self):
        import shutil
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            shutil.copytree(SOURCE, root / "skills/xp-infra")
            original = Path.cwd()
            os.chdir(root)
            try:
                for cmd in (["init", "-q"], ["add", "skills"],
                            ["-c", "user.name=Fixture", "-c", "user.email=a@example.test",
                             "-c", "commit.gpgsign=false", "commit", "-qm", "fixture"]):
                    subprocess.run(["git", *cmd], check=True)
                sha = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
                (root / "skills/xp-infra/private.env").write_text("SECRET_CANARY")
                first = MODULE.package("1.2.3-rc.1", sha, root / "first")
                second = MODULE.package("1.2.3-rc.1", sha, root / "second")
                self.assertEqual(first.read_bytes(), second.read_bytes())
                checksum = hashlib.sha256(first.read_bytes()).hexdigest()
                self.assertTrue(first.with_name(first.name + ".sha256").read_text()
                                .startswith(checksum))
                with tarfile.open(first) as archive:
                    archive.extractall(root / "installed", filter="data")
                skill = root / "installed/xp-infra"
                manifest = json.loads((skill / "manifest.json").read_text())
                self.assertEqual(manifest["source_sha"], sha)
                self.assertEqual(manifest["version"], "1.2.3-rc.1")
                self.assertFalse((skill / "private.env").exists())
                for name, digest in manifest["files"].items():
                    self.assertEqual(hashlib.sha256((skill / name).read_bytes()).hexdigest(),
                                     digest)
                out = subprocess.check_output(["bun", str(skill / "scripts/xp-infra"),
                                               "--help"], text=True)
                self.assertIn("xp-infra", out)
                with self.assertRaises(ValueError):
                    MODULE.package("1.2.3", "0" * 40, root / "wrong")
                os.symlink("/etc/passwd", root / "skills/xp-infra/scripts/link")
                subprocess.run(["git", "add", "skills"], check=True)
                subprocess.run(["git", "-c", "user.name=Fixture", "-c",
                                "user.email=a@example.test", "-c", "commit.gpgsign=false",
                                "commit", "-qm", "unsafe"], check=True)
                unsafe = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True)
                with self.assertRaises(ValueError):
                    MODULE.package("1.2.3", unsafe.strip(), root / "unsafe")
            finally:
                os.chdir(original)


if __name__ == "__main__":
    unittest.main()
