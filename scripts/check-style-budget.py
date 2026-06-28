#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from pathlib import Path


MAX_LINE_LENGTH = 100
MAX_SOURCE_LINES = 1000
BASELINE_PATH = Path("scripts/style-budget-baseline.json")

LINE_CHECK_SUFFIXES = {
    ".cjs",
    ".css",
    ".html",
    ".js",
    ".json",
    ".jsonc",
    ".md",
    ".mjs",
    ".rs",
    ".toml",
    ".ts",
    ".tsx",
    ".yaml",
    ".yml",
}

SOURCE_SIZE_SUFFIXES = {
    ".cjs",
    ".css",
    ".js",
    ".mjs",
    ".rs",
    ".ts",
    ".tsx",
}

SKIP_DIRS = {
    ".codex",
    ".git",
    ".storybook-static",
    "dist",
    "node_modules",
    "target",
}

SKIP_FILES = {
    "Cargo.lock",
    "bun.lock",
}

SKIP_SUFFIXES = {
    ".ico",
    ".jpeg",
    ".jpg",
    ".png",
    ".webp",
}


def should_skip(path: Path) -> bool:
    if path.name in SKIP_FILES:
        return True
    if path.suffix in SKIP_SUFFIXES:
        return True
    return any(part in SKIP_DIRS for part in path.parts)


def is_size_checked(path: Path) -> bool:
    if path.suffix not in SOURCE_SIZE_SUFFIXES:
        return False
    return True


def is_line_checked(path: Path) -> bool:
    return path.suffix in LINE_CHECK_SUFFIXES


def iter_files(root: Path):
    for path in root.rglob("*"):
        rel = path.relative_to(root)
        if path.is_file() and not should_skip(rel):
            yield path


def read_lines(path: Path) -> list[str] | None:
    try:
        return path.read_text(encoding="utf-8").splitlines()
    except UnicodeDecodeError:
        return None


def file_metrics(path: Path, lines: list[str]) -> dict[str, int]:
    long_lines = [len(line) for line in lines if len(line) > MAX_LINE_LENGTH]
    metrics = {
        "long_line_count": len(long_lines),
        "max_line_length": max((len(line) for line in lines), default=0),
    }
    if is_size_checked(path):
        metrics["source_lines"] = len(lines)
    return metrics


def load_baseline(root: Path) -> dict[str, dict[str, int]]:
    path = root / BASELINE_PATH
    if not path.exists():
        return {}
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict):
        raise SystemExit(f"invalid baseline shape: {path}")
    return data


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Check repository line-length and source file-size budgets."
    )
    parser.add_argument("--root", default=".", help="repository root")
    args = parser.parse_args()

    root = Path(args.root).resolve()
    baseline = load_baseline(root)
    failures: list[str] = []

    for path in iter_files(root):
        rel = path.relative_to(root)
        if path.suffix not in LINE_CHECK_SUFFIXES | SOURCE_SIZE_SUFFIXES:
            continue

        lines = read_lines(path)
        if lines is None:
            continue
        metrics = file_metrics(rel, lines)
        rel_key = rel.as_posix()
        budget = baseline.get(rel_key, {})

        if is_size_checked(rel):
            source_limit = int(budget.get("source_lines", MAX_SOURCE_LINES))
            if metrics["source_lines"] > source_limit:
                failures.append(
                    f"{rel}: file has {metrics['source_lines']} lines, "
                    f"limit is {source_limit}"
                )

        if not is_line_checked(rel):
            continue

        allowed_long_lines = int(budget.get("long_line_count", 0))
        allowed_max_line = int(budget.get("max_line_length", MAX_LINE_LENGTH))
        if metrics["long_line_count"] > allowed_long_lines:
            failures.append(
                f"{rel}: has {metrics['long_line_count']} long lines, "
                f"limit is {allowed_long_lines}"
            )
        if metrics["max_line_length"] > allowed_max_line:
            failures.append(
                f"{rel}: max line has {metrics['max_line_length']} characters, "
                f"limit is {allowed_max_line}"
            )

    if failures:
        print("style budget check failed:")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print(
        "style budget check passed: "
        f"new files line_length<={MAX_LINE_LENGTH}, source_lines<={MAX_SOURCE_LINES}; "
        "baseline entries did not grow"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
