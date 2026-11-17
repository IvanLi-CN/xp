"""Pure identity, applicability and gate policy; no network or side effects."""

from datetime import datetime, timedelta, timezone


def related(path):
    return (
        path.startswith(
            (
                "crates/xp-proxy/",
                "crates/xp-proxy-core/",
                "scripts/perf/xp-proxy/",
                ".cargo/",
            )
        )
        or path
        in {
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain",
            "rust-toolchain.toml",
            ".github/workflows/ci-performance.yml",
            "scripts/e2e/install-mihomo-v1.19.29.sh",
        }
        or path.startswith(".github/workflows/xp-proxy-perf")
        or path.startswith(".github/scripts/xp-proxy-perf")
    )


def gate(*, applicable, waived, measurement, current_sha, measured_sha):
    if waived:
        return (
            "success",
            "waived: human merge exception; measurement continues unchanged",
        )
    if not applicable:
        return "success", "not applicable: no proxy inputs changed"
    if current_sha != measured_sha:
        return "pending", "waiting for current candidate measurement"
    if measurement == "success":
        return "success", "current automatic light measurement passed"
    if measurement in {"failure", "error", "cancelled", "timed_out", "skipped"}:
        return "failure", f"measurement {measurement}; no human merge exception"
    return "pending", "waiting for automatic light measurement"


def validate_receipt(
    receipt, run, *, repository_id, workflow_id, default_branch, now=None
):
    now = now or datetime.now(timezone.utc)
    if run["repository"]["id"] != repository_id or run["workflow_id"] != workflow_id:
        raise ValueError("untrusted receipt repository/workflow")
    if run["event"] != "workflow_dispatch" or run["head_branch"] != default_branch:
        raise ValueError("receipt must originate on default branch manual workflow")
    if (
        receipt["repository_id"] != repository_id
        or receipt["operation_run_id"] != run["id"]
    ):
        raise ValueError("receipt origin mismatch")
    if receipt["operation_run_attempt"] != run["run_attempt"]:
        raise ValueError("receipt attempt mismatch")
    if (
        receipt["workflow_sha"] != run["head_sha"]
        or receipt["workflow_path"] != run["path"].split("@")[0]
    ):
        raise ValueError("receipt workflow revision mismatch")
    if receipt["workflow_ref"] != f"refs/heads/{default_branch}":
        raise ValueError("receipt ref mismatch")
    if (
        receipt["actor"] != run["triggering_actor"]["login"]
        or not receipt["reason"].strip()
    ):
        raise ValueError("receipt actor/reason mismatch")
    created = datetime.fromisoformat(receipt["created_at"].replace("Z", "+00:00"))
    if not timedelta(0) <= now - created < timedelta(days=90):
        raise ValueError("receipt expired or future dated")
    if receipt["action"] not in {"invalidate", "restore"}:
        raise ValueError("invalid disposition action")
    return receipt


def disposition(receipts, target):
    # The uploader commits the decision. A failed Check projection never rolls it back.
    chain = sorted(
        [r for r in receipts if tuple(r["target_identity"]) == tuple(target)],
        key=lambda r: r["sequence"],
    )
    previous = None
    for sequence, receipt in enumerate(chain, 1):
        if receipt["sequence"] != sequence or receipt["previous_digest"] != previous:
            raise ValueError("missing or broken audit chain; result cannot be reused")
        previous = receipt["digest"]
    return (
        ("invalidated" if chain[-1]["action"] == "invalidate" else "valid")
        if chain
        else "valid"
    )
