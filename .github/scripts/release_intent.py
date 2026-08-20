#!/usr/bin/env python3
"""Resolve release intent from merge-time GitHub events or explicit backfill input."""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from dataclasses import dataclass
from typing import Any
from urllib import error, request

TYPE_LABELS = {
    "type:docs": "docs",
    "type:skip": "skip",
    "type:patch": "patch",
    "type:minor": "minor",
    "type:major": "major",
}
CHANNEL_LABELS = {"channel:stable": "stable", "channel:prerelease": "prerelease"}
VERSION_RE = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+(?:-rc\.[0-9]+)?$")
SHA_RE = re.compile(r"^[0-9a-f]{40}$")


class IntentError(RuntimeError):
    """A fail-closed release intent error."""


class GitHubApi:
    def __init__(self, root: str, token: str):
        self.root = root.rstrip("/")
        self.token = token

    def get_json_with_headers(self, path: str) -> tuple[Any, dict[str, str]]:
        url = path if path.startswith("http") else f"{self.root}{path}"
        req = request.Request(
            url,
            headers={
                "Accept": "application/vnd.github+json",
                "Authorization": f"Bearer {self.token}",
                "X-GitHub-Api-Version": "2022-11-28",
                "User-Agent": "xp-release-intent-resolver",
            },
        )
        try:
            with request.urlopen(req, timeout=20) as response:
                payload = json.loads(response.read().decode("utf-8"))
                headers = {
                    str(key).lower(): str(value)
                    for key, value in response.headers.items()
                }
                return payload, headers
        except (OSError, ValueError, error.HTTPError) as exc:
            raise IntentError(f"github_api_failure:{path}:{type(exc).__name__}") from exc

    def get_json(self, path: str) -> Any:
        payload, _ = self.get_json_with_headers(path)
        return payload

    def get_pages(self, path: str) -> list[Any]:
        separator = "&" if "?" in path else "?"
        page = 1
        values: list[Any] = []
        while True:
            payload, headers = self.get_json_with_headers(
                f"{path}{separator}per_page=100&page={page}"
            )
            if not isinstance(payload, list):
                raise IntentError(f"github_api_shape_failure:{path}")
            values.extend(payload)
            link = headers.get("link", "")
            has_next = any('rel="next"' in part for part in link.split(","))
            if has_next:
                page += 1
                continue
            if 'rel="last"' in link or (not link and len(payload) < 100):
                return values
            if len(payload) >= 100:
                raise IntentError(f"github_api_pagination_incomplete:{path}:page={page}")
            raise IntentError(f"github_api_pagination_incomplete:{path}:link={link}")


@dataclass(frozen=True)
class PullRequest:
    number: int
    url: str
    merge_commit_sha: str
    created_at: str
    merged_at: str
    base_ref: str


def _event_sort_key(event: dict[str, Any]) -> tuple[int, str]:
    event_id = event.get("id")
    if isinstance(event_id, int) or (isinstance(event_id, str) and event_id.isdigit()):
        return (int(event_id), "")
    return (0, str(event.get("created_at") or ""))


def find_merge_event(events: list[dict[str, Any]], merge_commit_sha: str) -> dict[str, Any]:
    merged_events = [event for event in events if event.get("event") == "merged"]
    if merged_events:
        if len(merged_events) != 1:
            raise IntentError(f"invalid_merge_event_count:{len(merged_events)}")
        return merged_events[0]

    closed_events = [
        event
        for event in events
        if event.get("event") == "closed" and event.get("commit_id") == merge_commit_sha
    ]
    if len(closed_events) != 1:
        raise IntentError(f"invalid_merge_event_count:{len(closed_events)}")
    return closed_events[0]


def labels_at_merge(events: list[dict[str, Any]], merge_commit_sha: str) -> set[str]:
    if not events or not all(isinstance(event, dict) for event in events):
        raise IntentError("invalid_event_history")
    ordered = sorted(events, key=_event_sort_key)
    merge_event = find_merge_event(ordered, merge_commit_sha)
    merge_key = _event_sort_key(merge_event)
    labels: set[str] = set()
    for event in ordered:
        if _event_sort_key(event) > merge_key:
            break
        label = event.get("label")
        name = label.get("name") if isinstance(label, dict) else None
        if not isinstance(name, str) or not name:
            continue
        if event.get("event") == "labeled":
            labels.add(name)
        elif event.get("event") == "unlabeled":
            labels.discard(name)
    return labels


def verify_event_history(events: list[dict[str, Any]], pull_request: PullRequest) -> None:
    timestamps = [
        str(event.get("created_at") or "")
        for event in events
        if isinstance(event, dict) and event.get("created_at")
    ]
    if len(timestamps) != len(events) or min(timestamps) > pull_request.created_at:
        raise IntentError("incomplete_event_history")


def validate_labels(labels: set[str]) -> tuple[str, str, bool, str, str]:
    type_like = sorted(name for name in labels if name.startswith("type:"))
    unknown_types = sorted(name for name in type_like if name not in TYPE_LABELS)
    if unknown_types:
        raise IntentError(f"unknown_type_label:{','.join(unknown_types)}")
    if len(type_like) != 1:
        raise IntentError(f"invalid_type_label_count:{len(type_like)}")

    channel_like = sorted(name for name in labels if name.startswith("channel:"))
    unknown_channels = sorted(name for name in channel_like if name not in CHANNEL_LABELS)
    if unknown_channels:
        raise IntentError(f"unknown_channel_label:{','.join(unknown_channels)}")
    if len(channel_like) != 1:
        raise IntentError(f"invalid_channel_label_count:{len(channel_like)}")

    type_label = type_like[0]
    channel_label = channel_like[0]
    release_type = TYPE_LABELS[type_label]
    channel = CHANNEL_LABELS[channel_label]
    should_release = release_type not in {"docs", "skip"}
    return (
        release_type,
        channel,
        should_release,
        type_label,
        "intent_release" if should_release else "intent_skip",
    )


def validate_manual_inputs(
    release_type: str, channel: str, expected_version: str, reason: str
) -> None:
    if release_type not in {"patch", "minor", "major"}:
        raise IntentError(f"invalid_release_type:{release_type or '<empty>'}")
    if channel not in {"stable", "prerelease"}:
        raise IntentError(f"invalid_channel:{channel or '<empty>'}")
    if not VERSION_RE.fullmatch(expected_version):
        raise IntentError("invalid_expected_version")
    has_rc = "-rc." in expected_version
    if (channel == "stable" and has_rc) or (channel == "prerelease" and not has_rc):
        raise IntentError("expected_version_channel_mismatch")
    if not reason or len(reason) > 240 or any(ord(char) < 32 or ord(char) == 127 for char in reason):
        raise IntentError("invalid_backfill_reason")


def ensure_target_on_main(sha: str) -> None:
    if not SHA_RE.fullmatch(sha):
        raise IntentError("invalid_head_sha")
    try:
        subprocess.run(
            ["git", "fetch", "--quiet", "origin", "main"],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
        )
        subprocess.run(
            ["git", "cat-file", "-e", f"{sha}^{{commit}}"],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
        )
        subprocess.run(
            ["git", "merge-base", "--is-ancestor", sha, "origin/main"],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
        )
    except subprocess.CalledProcessError as exc:
        detail = (exc.stderr or "").strip().replace("\n", " ")[:160]
        raise IntentError(f"head_sha_not_on_main:{detail or 'not_an_ancestor'}") from exc


def resolve_pull(api: GitHubApi, repository: str, sha: str) -> PullRequest:
    summaries = api.get_pages(f"/repos/{repository}/commits/{sha}/pulls")
    candidates: list[PullRequest] = []
    for summary in summaries:
        number = summary.get("number") if isinstance(summary, dict) else None
        if not isinstance(number, int):
            continue
        detail = api.get_json(f"/repos/{repository}/pulls/{number}")
        if not isinstance(detail, dict):
            continue
        if (
            detail.get("merged_at")
            and detail.get("merge_commit_sha") == sha
            and detail.get("base", {}).get("ref") == "main"
        ):
            candidates.append(
                PullRequest(
                    number=number,
                    url=str(detail.get("html_url") or ""),
                    merge_commit_sha=sha,
                    created_at=str(detail.get("created_at") or ""),
                    merged_at=str(detail["merged_at"]),
                    base_ref="main",
                )
            )
    if len(candidates) != 1:
        raise IntentError(f"invalid_merged_pr_count:{len(candidates)}")
    return candidates[0]


def output_values(values: dict[str, str]) -> None:
    for key, value in values.items():
        print(f"{key}={value}")
    output_path = os.environ.get("GITHUB_OUTPUT")
    if output_path:
        with open(output_path, "a", encoding="utf-8") as handle:
            for key, value in values.items():
                handle.write(f"{key}={value}\n")


def run() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", choices=("auto", "manual"), default="auto")
    parser.add_argument("--repository", default=os.environ.get("GITHUB_REPOSITORY", ""))
    parser.add_argument("--token", default=os.environ.get("GITHUB_TOKEN", ""))
    parser.add_argument("--api-root", default=os.environ.get("GITHUB_API_URL", "https://api.github.com"))
    parser.add_argument("--sha", default=os.environ.get("WORKFLOW_RUN_SHA", os.environ.get("GITHUB_SHA", "")))
    parser.add_argument("--release-type", default=os.environ.get("RELEASE_TYPE", ""))
    parser.add_argument("--channel", default=os.environ.get("RELEASE_CHANNEL", ""))
    parser.add_argument("--expected-version", default=os.environ.get("EXPECTED_VERSION", ""))
    parser.add_argument("--reason", default=os.environ.get("RELEASE_REASON", ""))
    args = parser.parse_args()

    try:
        if not args.repository or not args.token:
            raise IntentError("missing_github_context")
        ensure_target_on_main(args.sha)
        api = GitHubApi(args.api_root, args.token)
        pr = resolve_pull(api, args.repository, args.sha)

        if args.mode == "manual":
            validate_manual_inputs(args.release_type, args.channel, args.expected_version, args.reason)
            values = {
                "should_release": "true",
                "bump_level": args.release_type,
                "release_intent_label": f"type:{args.release_type}",
                "release_intent_type": args.release_type,
                "release_channel": args.channel,
                "is_prerelease": "true" if args.channel == "prerelease" else "false",
                "intent_source": "manual_backfill",
                "pr_number": str(pr.number),
                "pr_url": pr.url,
                "reason": "manual_backfill",
                "backfill_reason": args.reason,
                "expected_version": args.expected_version,
            }
        else:
            events = api.get_pages(f"/repos/{args.repository}/issues/{pr.number}/events")
            verify_event_history(events, pr)
            label_set = labels_at_merge(events, pr.merge_commit_sha)
            release_type, channel, should_release, type_label, reason = validate_labels(label_set)
            values = {
                "should_release": "true" if should_release else "false",
                "bump_level": release_type if should_release else "",
                "release_intent_label": type_label,
                "release_intent_type": release_type,
                "release_channel": channel,
                "is_prerelease": "true" if channel == "prerelease" else "false",
                "intent_source": "merge_event",
                "pr_number": str(pr.number),
                "pr_url": pr.url,
                "reason": reason,
                "backfill_reason": "",
                "expected_version": "",
            }

        print(
            "Release intent: "
            f"source={values['intent_source']} sha={args.sha} pr=#{values['pr_number']} "
            f"type={values['release_intent_type']} channel={values['release_channel']} "
            f"should_release={values['should_release']}"
        )
        if values["backfill_reason"]:
            print(f"backfill_reason={values['backfill_reason']}")
        output_values(values)
        return 0
    except IntentError as exc:
        print(f"release-intent: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(run())
