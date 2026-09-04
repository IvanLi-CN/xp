# XP Web Console Context

This glossary defines the terms for the browser-based XP administration console.
It describes trust and routing meaning, not React components or HTTP implementation.

## Backend Selection

**Primary Backend**:
The one verified XP Node origin to which the current browser profile directly sends
all console API and status-stream requests. It remains the browser's only direct
control-plane entry while XP performs peer coordination on the server side.
_Avoid_: active node, leader, proxy target

**Backend Candidate**:
A persisted, same-cluster Node origin observed from an authenticated node inventory
and eligible to become the Primary Backend.
_Avoid_: arbitrary URL, failover URL, endpoint

**Origin Allowlist**:
The exact HTTPS origins of the current cluster's registered Nodes that an XP node
accepts as browser origins for cross-origin console requests.
_Avoid_: permissive CORS, wildcard CORS, browser trust list

**Backend Profile**:
The browser-local selection state for one cluster, including its cluster identity,
Primary Backend, and Backend Candidates.
_Avoid_: global backend setting, shared session

**Backend Switch Barrier**:
The temporary condition that prevents changing the Primary Backend while a console
write request is unresolved. A timed-out write becomes an explicitly unknown result
before the barrier is released.
_Avoid_: automatic replay, transparent failover

**Legacy Node Navigation**:
The existing full-page jump from one node-hosted console origin to another using its
separate login handoff. It is compatibility behaviour, not Backend Selection.
_Avoid_: primary-backend switch, failover control
