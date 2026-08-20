# XP Release Context

This glossary defines the terms used when deciding which merged change becomes an XP release.
It deliberately describes the domain language, not the workflow implementation.

## Release intent

**Release Intent**:
The approved classification of a merged change: whether it produces a release,
which version increment applies, and which release channel is allowed.
_Avoid_: release trigger, current PR labels

**Merge-time Intent**:
The Release Intent reconstructed from the labels and events that existed before
the pull request was merged.
_Avoid_: current label state

**Release Target**:
The exact commit that a release tag, GitHub Release, and container image represent.
_Avoid_: workflow head, latest main

**Manual Backfill**:
An owner-approved release of an already merged Release Target through explicit
type, channel, version, and reason inputs.
_Avoid_: relabel-and-rerun, retroactive PR edit
