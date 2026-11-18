#!/usr/bin/env python3
"""Package only committed XP skill sources with a source-bound manifest."""
import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import re
import subprocess
import tarfile


def package(version: str, source_sha: str, output: Path) -> Path:
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?", version):
        raise ValueError("invalid version")
    if not re.fullmatch(r"[0-9a-f]{40}", source_sha):
        raise ValueError("invalid source SHA")
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    if head != source_sha:
        raise ValueError("source SHA does not match checkout")
    tree = subprocess.check_output([
        "git", "ls-tree", "-r", source_sha, "--", "skills/xp-infra",
    ], text=True)
    members = {}
    modes = {}
    for row in tree.splitlines():
        meta, name = row.split("\t", 1)
        mode, kind, blob = meta.split()
        relative = name.removeprefix("skills/xp-infra/")
        allowed = relative in {"SKILL.md", "LICENSE", "ORIGIN.md"} or relative.startswith(
            ("agents/", "scripts/", "references/", "tests/")
        )
        if not allowed or mode not in {"100644", "100755"} or kind != "blob":
            raise ValueError("unexpected or unsafe skill source")
        if relative.endswith((".env", ".pyc")) or ".." in Path(relative).parts:
            raise ValueError("private or generated source in skill")
        members[relative] = subprocess.check_output(["git", "cat-file", "blob", blob])
        modes[relative] = 0o755 if mode == "100755" else 0o644
    if not {"SKILL.md", "LICENSE", "ORIGIN.md", "scripts/xp-infra"} <= members.keys():
        raise ValueError("incomplete skill source")
    manifest = {
        "schema_version": 1, "name": "xp-infra", "version": version,
        "source_repository": "IvanLi-CN/xp", "source_sha": source_sha,
        "dependencies": {"operator": ["Bun", "OpenSSH", "curl"],
                         "node": ["xp-ops runtime status --json", "POSIX tools"]},
        "compatibility": {"runtimes": ["xray", "xp-proxy"],
                          "unsupported_observations": "fail-closed"},
        "files": {name: hashlib.sha256(data).hexdigest()
                  for name, data in sorted(members.items())},
    }
    members["manifest.json"] = (json.dumps(manifest, indent=2) + "\n").encode()
    output.mkdir(parents=True, exist_ok=True)
    archive = output / f"xp-infra-skill-v{version}.tar.gz"
    with archive.open("xb") as raw:
        with gzip.GzipFile(fileobj=raw, mode="wb", filename="", mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w") as tar:
                for name, data in sorted(members.items()):
                    member = tarfile.TarInfo("xp-infra/" + name)
                    member.size = len(data)
                    member.mode = modes.get(name, 0o644)
                    tar.addfile(member, io.BytesIO(data))
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    archive.with_name(archive.name + ".sha256").write_text(f"{digest}  {archive.name}\n")
    return archive


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--source-sha", required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    print(package(args.version, args.source_sha, args.output_dir))
