#!/usr/bin/env python3
"""Check the release failure notifier's reusable-workflow contract."""

from __future__ import annotations

import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github" / "workflows" / "notify-release-failure.yml"
OIDRUNE_REF = (
    "IvanLi-CN/oidrune/.github/workflows/notify.yml@"
    "e48822f99c6402a753ed86557ea029754cbab20b"
)
LEGACY_REF = "IvanLi-CN/github-workflows/.github/workflows/release-failure-telegram.yml@main"


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def job_block(source: str, name: str, next_name: str | None = None) -> str:
    end = rf"(?=^  {re.escape(next_name)}:|\Z)" if next_name else r"\Z"
    match = re.search(rf"^  {re.escape(name)}:\n(.*?){end}", source, re.MULTILINE | re.DOTALL)
    require(match is not None, f"missing {name} job")
    return match.group(1)


source = WORKFLOW.read_text(encoding="utf-8")
require(LEGACY_REF not in source, "legacy reusable workflow reference remains")
require(source.count(f"uses: {OIDRUNE_REF}") == 2, "expected exactly two pinned Oidrune calls")
require("SHOUTRRR_URL" not in source, "legacy Telegram secret wiring remains")
require("secrets:" not in source, "reusable workflow still receives caller secrets")
require("gateway_url" not in source, "caller overrides the Oidrune gateway")
require("oidc_audience" not in source, "caller overrides the OIDC audience")
require(re.search(r"^permissions:\n  id-token: write\n", source, re.MULTILINE) is not None, "caller must grant id-token write")

workflow_run = """  workflow_run:
    workflows:
      - release
    types:
      - completed
    branches:
      - main
"""
require(workflow_run in source, "release workflow_run filter changed")
require("  workflow_dispatch:\n" in source, "manual workflow_dispatch trigger is missing")
failure_condition = "if: ${{ github.event_name == 'workflow_run' && github.event.workflow_run.conclusion == 'failure' }}"
require(source.count(failure_condition) == 2, "release failure condition must guard both jobs")
require("  notify_failure:\n" in source, "failure notifier job is missing")
require("    needs:\n      - resolve_release_context\n" in source, "failure notifier metadata resolver dependency changed")
require("  smoke_test:\n    if: ${{ github.event_name == 'workflow_dispatch' }}\n" in source, "smoke job dispatch condition changed")

failure_job = job_block(source, "notify_failure", "smoke_test")
smoke_job = job_block(source, "smoke_test")
require(failure_condition in failure_job, "notify_failure failure condition is not attached to its job")
require(failure_condition not in smoke_job, "smoke_test must not inherit the release failure condition")
require(
    "if: ${{ github.event_name == 'workflow_dispatch' }}" in smoke_job,
    "smoke_test dispatch condition is not attached to its job",
)
for name, block in (("notify_failure", failure_job), ("smoke_test", smoke_job)):
    require("uses: " + OIDRUNE_REF in block, f"{name} does not call pinned Oidrune")
    require("outcome: failure\n" in block, f"{name} does not declare outcome")
    require("summary: |\n" in block, f"{name} does not provide a multiline summary")
    for field in (
        "status:",
        "project: ${{ github.repository }}",
        "workflow:",
        "target SHA:",
        "run URL:",
    ):
        require(field in block, f"{name} summary is missing {field}")

require("smoke: workflow_dispatch" in smoke_job, "smoke summary title/context is missing")
require("🚨 Release Failed · ${{ github.repository }}\n        status: failure" in failure_job, "failure summary title/status changed")
require("🧪 Smoke Test · ${{ github.repository }}\n        status: smoke test" in smoke_job, "smoke summary title/status changed")
print("notify-release-failure workflow contract: ok")
