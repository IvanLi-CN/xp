#!/usr/bin/env python3
"""GitHub metadata adapter. All hosted operations use the bound gh backend."""

import argparse
from datetime import datetime, timezone
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import zipfile
import tomllib
from policy import related, validate_receipt, disposition


def api(path, payload=None, method="POST"):
    args = ["gh", "api", path]
    if payload is not None:
        args += ["--method", method, "--input", "-"]
    return json.loads(
        subprocess.check_output(
            args, input=json.dumps(payload).encode() if payload is not None else None
        )
    )


def pages(path, key=None):
    result = []
    for page in range(1, 1001):
        data = api(path + ("&" if "?" in path else "?") + f"per_page=100&page={page}")
        values = data[key] if key else data
        result.extend(values)
        if len(values) < 100:
            return result
    raise ValueError("API pagination bound exceeded")


def artifact_file(repo, artifact, filename):
    if artifact["expired"]:
        raise ValueError("artifact expired; fail closed")
    data = subprocess.check_output(
        ["gh", "api", f"repos/{repo}/actions/artifacts/{artifact['id']}/zip"]
    )
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        if (
            filename not in archive.namelist()
            or archive.getinfo(filename).file_size > 16 * 1024 * 1024
        ):
            raise ValueError("invalid metadata artifact")
        return json.loads(archive.read(filename))


def seal(value):
    return hashlib.sha256(
        json.dumps(
            {k: v for k, v in value.items() if k != "digest"},
            sort_keys=True,
            separators=(",", ":"),
        ).encode()
    ).hexdigest()


def receipts(repo, repo_id, workflow_id, branch, target):
    results = []
    runs = pages(
        f"repos/{repo}/actions/workflows/{workflow_id}/runs?event=workflow_dispatch",
        "workflow_runs",
    )
    for run in runs:
        # Unrelated branch executions have no authority, even if names match.
        if run["head_branch"] != branch:
            continue
        artifacts = pages(
            f"repos/{repo}/actions/runs/{run['id']}/artifacts", "artifacts"
        )
        for artifact in artifacts:
            if not artifact["name"].startswith(
                f"xp-perf-disposition-{target[1]}-{target[2]}-"
            ):
                continue
            # An expired audit record may still protect a retained target. Never turn
            # that uncertainty into 'no mark'. Consumers reject reuse instead.
            receipt = artifact_file(repo, artifact, "receipt.json")
            attempt = api(
                f"repos/{repo}/actions/runs/{run['id']}/attempts/{receipt['operation_run_attempt']}"
            )
            validate_receipt(
                receipt,
                attempt,
                repository_id=repo_id,
                workflow_id=workflow_id,
                default_branch=branch,
            )
            if receipt["digest"] != seal(receipt):
                raise ValueError("receipt digest mismatch")
            results.append(receipt)
    return results


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "command",
        choices=[
            "classify",
            "manifest",
            "verify-artifacts",
            "prepare-mark",
            "project-mark",
        ],
    )
    args = parser.parse_args()
    repo = os.environ["GITHUB_REPOSITORY"]
    if args.command == "classify":
        event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text())
        candidate = api(f"repos/{repo}/commits/{os.environ['CANDIDATE_SHA']}")["sha"]
        baseline = api(f"repos/{repo}/commits/main")["sha"]
        if "pull_request" in event:
            paths = [
                f["filename"]
                for f in pages(f"repos/{repo}/pulls/{event['number']}/files")
            ]
            applicable = any(related(p) for p in paths)
        elif os.environ["GITHUB_EVENT_NAME"] == "workflow_dispatch":
            applicable = True
        else:
            before = event.get("before")
            if not before or before == "0" * 40:
                applicable = True
            else:
                # Compare API truncates at 300 files: conservatively run on that boundary.
                files = api(f"repos/{repo}/compare/{before}...{candidate}")["files"]
                applicable = len(files) >= 300 or any(
                    related(f["filename"]) for f in files
                )
        with open(os.environ["GITHUB_OUTPUT"], "a") as output:
            output.write(
                f"candidate={candidate}\nbaseline={baseline}\napplicable={str(applicable).lower()}\n"
            )
    elif args.command == "manifest":
        config = Path("tools/scripts/perf/xp-proxy/config.json")
        output = Path("bundle")
        build_config = (
            tomllib.loads(Path("source/Cargo.toml").read_text())
            .get("profile", {})
            .get("release", {})
        )
        value = dict(
            build_config=build_config,
            schema_version=1,
            repository_id=int(os.environ["GITHUB_REPOSITORY_ID"]),
            sha=os.environ["SOURCE_SHA"],
            target="x86_64-unknown-linux-musl",
            profile="release",
            toolchain="1.96.0",
            config_digest=hashlib.sha256(config.read_bytes()).hexdigest(),
            workflow_ref=os.environ["GITHUB_WORKFLOW_REF"],
            run_id=int(os.environ["GITHUB_RUN_ID"]),
            run_attempt=int(os.environ["GITHUB_RUN_ATTEMPT"]),
            artifact_id=int(os.environ["BINARY_ARTIFACT_ID"]),
            artifact_name=os.environ["BINARY_ARTIFACT_NAME"],
            checksums={
                p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                for p in output.iterdir()
                if p.is_file()
            },
        )
        Path("manifest").mkdir(exist_ok=True)
        Path("manifest/manifest.json").write_text(json.dumps(value, indent=2))
    elif args.command == "verify-artifacts":
        for role in ("candidate", "main"):
            value = json.loads(Path(role, "manifest.json").read_text())
            artifact = api(f"repos/{repo}/actions/artifacts/{value['artifact_id']}")
            if (
                value["repository_id"] != int(os.environ["GITHUB_REPOSITORY_ID"])
                or value["run_id"] != int(os.environ["GITHUB_RUN_ID"])
                or value["run_attempt"] != int(os.environ["GITHUB_RUN_ATTEMPT"])
            ):
                raise ValueError("producer run/attempt mismatch")
            if value["workflow_ref"] != os.environ["GITHUB_WORKFLOW_REF"]:
                raise ValueError("producer workflow mismatch")
            if (
                artifact["workflow_run"]["id"] != value["run_id"]
                or artifact["name"] != value["artifact_name"]
                or artifact["expired"]
            ):
                raise ValueError("artifact origin mismatch")
        if (
            json.loads(Path("candidate/manifest.json").read_text())["build_config"]
            != json.loads(Path("main/manifest.json").read_text())["build_config"]
        ):
            raise ValueError("candidate/main build configurations differ")
    elif args.command == "prepare-mark":
        details = api(f"repos/{repo}")
        branch = details["default_branch"]
        if (
            os.environ["GITHUB_REF"] != f"refs/heads/{branch}"
            or os.environ["GITHUB_EVENT_NAME"] != "workflow_dispatch"
        ):
            raise ValueError("manual default-branch workflow required")
        run_id = int(os.environ["TARGET_RUN_ID"])
        attempt = int(os.environ["TARGET_RUN_ATTEMPT"])
        target = api(f"repos/{repo}/actions/runs/{run_id}/attempts/{attempt}")
        workflow = api(f"repos/{repo}/actions/workflows/xp-proxy-perf.yml")
        if (
            target["workflow_id"] != workflow["id"]
            or target["repository"]["id"] != details["id"]
            or target["head_branch"] != branch
            or target["event"] not in {"push", "workflow_dispatch"}
        ):
            raise ValueError("target must be this repository's main performance result")
        artifacts = pages(f"repos/{repo}/actions/runs/{run_id}/artifacts", "artifacts")
        artifact = next(
            a for a in artifacts if a["name"] == f"xp-proxy-perf-{run_id}-{attempt}"
        )
        measurement = artifact_file(repo, artifact, "measurement.json")
        if measurement["candidate_sha"] != target["head_sha"]:
            raise ValueError("measurement SHA mismatch")
        own = api(f"repos/{repo}/actions/runs/{os.environ['GITHUB_RUN_ID']}")
        identity = [details["id"], run_id, attempt]
        prior = receipts(repo, details["id"], own["workflow_id"], branch, identity)
        chain = [r for r in prior if r["target_identity"] == identity]
        disposition(chain, identity)
        # Rerunning a dispatch repairs projection, never appends another decision.
        existing = next((r for r in chain if r["operation_run_id"] == own["id"]), None)
        if existing:
            value = existing
        else:
            value = dict(
                schema_version=1,
                repository_id=details["id"],
                target_identity=identity,
                target_sha=target["head_sha"],
                target_workflow_id=target["workflow_id"],
                profile=measurement["profile"],
                target="x86_64-unknown-linux-musl",
                manifests=measurement["manifests"],
                measurement=measurement["measurement"],
                measurement_artifact_id=artifact["id"],
                measurement_digest=hashlib.sha256(
                    json.dumps(measurement, sort_keys=True).encode()
                ).hexdigest(),
                action=os.environ["MARK_ACTION"],
                reason=os.environ["MARK_REASON"],
                actor=os.environ["GITHUB_TRIGGERING_ACTOR"],
                created_at=datetime.now(timezone.utc).isoformat(),
                operation_run_id=own["id"],
                operation_run_attempt=int(os.environ["GITHUB_RUN_ATTEMPT"]),
                workflow_sha=own["head_sha"],
                workflow_path=own["path"].split("@")[0],
                workflow_ref=os.environ["GITHUB_REF"],
                sequence=len(chain) + 1,
                previous_digest=max(chain, key=lambda r: r["sequence"])["digest"]
                if chain
                else None,
                operation_url=own["html_url"],
                target_url=target["html_url"],
                retention_days=90,
            )
            if not value["reason"].strip():
                raise ValueError("reason is required")
            value["digest"] = seal(value)
        Path("receipt.json").write_text(json.dumps(value, indent=2))
        with open(os.environ["GITHUB_OUTPUT"], "a") as output:
            output.write(f"receipt_attempt={value['operation_run_attempt']}\n")
    else:
        value = json.loads(Path("receipt.json").read_text())
        run = api(
            f"repos/{repo}/actions/runs/{value['operation_run_id']}/attempts/{value['operation_run_attempt']}"
        )
        details = api(f"repos/{repo}")
        validate_receipt(
            value,
            run,
            repository_id=details["id"],
            workflow_id=api(f"repos/{repo}/actions/workflows/xp-proxy-perf-result.yml")[
                "id"
            ],
            default_branch=details["default_branch"],
        )
        if value["digest"] != seal(value):
            raise ValueError("receipt digest mismatch")
        target_chain = receipts(
            repo,
            details["id"],
            run["workflow_id"],
            details["default_branch"],
            value["target_identity"],
        )
        disposition(target_chain, value["target_identity"])
        if not any(r["digest"] == value["digest"] for r in target_chain):
            raise ValueError("receipt upload not committed")
        # Replaying an older operation must not overwrite a later human decision.
        value = max(target_chain, key=lambda r: r["sequence"])
        checks = pages(
            f"repos/{repo}/commits/{value['target_sha']}/check-runs", "check_runs"
        )
        identity = "xp-perf-disposition:" + ":".join(map(str, value["target_identity"]))
        matching = [c for c in checks if c.get("external_id") == identity]
        payload = dict(
            name="XP performance result disposition",
            head_sha=value["target_sha"],
            external_id=identity,
            status="completed",
            conclusion="neutral",
            details_url=value["operation_url"],
            output=dict(
                title=f"{'invalidated' if value['action'] == 'invalidate' else 'valid (restored)'} — run {value['target_identity'][1]} attempt {value['target_identity'][2]}",
                summary=f"XP human disposition; original measurement: {value['measurement']}\n\nActor: {value['actor']}\n\nReason: {value['reason']}\n\nReceipt SHA256: {value['digest']}\n\n[Original run]({value['target_url']}) · [Auditable operation]({value['operation_url']})",
            ),
        )
        if matching:
            payload.pop("head_sha")
            api(f"repos/{repo}/check-runs/{matching[0]['id']}", payload, method="PATCH")
        else:
            api(f"repos/{repo}/check-runs", payload)


if __name__ == "__main__":
    main()
