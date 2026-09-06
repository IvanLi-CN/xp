# XP Web Console Context

This glossary defines the terms for the browser-based XP administration console.
It describes trust and routing meaning, not React components or HTTP implementation.

## Static Delivery

**Static Web Console**:
The XP administration console delivered as a versioned static app shell from an
origin independent of any XP Node. It directly calls a selected Primary Backend;
it is neither a reverse proxy nor an additional control-plane server. Its first
release can be compatibility-pending until its Bootstrap Origin adopts the Static
Console-Compatible Node contract; this one-time migration does not make later
Web-only releases wait for Node upgrades.
_Avoid_: hosted node UI, API gateway, edge proxy

**Static Console Release Asset**:
The checksum-verified, immutable complete static app shell built for one XP release.
The release workflow deploys this exact artifact to the production static origin
before publishing node-upgrade artifacts. Its release archive and checksum are the
recovery source, rather than an arbitrary later checkout or static-hosting history.
_Avoid_: current main build, unversioned web folder, deferred activation asset

**Console Runtime Policy**:
A short-lived, versioned, authenticated description of the exact API origins an
already-built Static Web Console may connect to. It changes independently from the
app-shell build when cluster topology changes, includes only Static
Console-Compatible Nodes, and never becomes an anonymous discovery document.
_Avoid_: frontend configuration file, unrestricted discovery, deployment config

**Console Bootstrap Origin**:
The stable HTTPS origin embedded in the Static Web Console's initial trust policy
from which the browser retrieves a Console Runtime Policy. It is a discovery
anchor for authenticated discovery, not an automatically selected Primary Backend
or a public node-inventory endpoint.
_Avoid_: arbitrary seed node, failover target, API wildcard

**Applied Runtime Policy**:
The Console Runtime Policy version currently governing a controlled browser client.
It lets the client distinguish an app-shell update from a policy-only update and
determine whether a document reload is required before a newly allowed API origin
can be used.
_Avoid_: app version, cache version, browser session

**Policy Grant**:
The non-persistent, per-controlled-client record allowing the Service Worker to
apply an Applied Runtime Policy to that client's next navigation. It is removed on
logout or client loss, so an anonymous page never receives a prior user's policy.
_Avoid_: shared policy cache, browser credential, permanent Service Worker state

**Bootstrap Outage**:
The loss of the Console Bootstrap Origin. A client holding a Policy Grant may
continue to use an already allowed, healthy Primary Backend; a fresh or logged-out
client fails closed rather than probing arbitrary Node origins.
_Avoid_: automatic node fallback, public topology discovery, cluster outage

**Static Console-Compatible Node**:
An XP Node that admits the exact Console Origin Allowlist through browser CORS and
serves authenticated Console Runtime Policy. Only these Nodes may appear as Static
Web Console Backend Candidates. A legacy Node remains available through its
node-hosted console, but the Static Web Console neither probes nor selects it.
_Avoid_: inferred compatibility, legacy CORS fallback, anonymous capability check

## Backend Selection

**Primary Backend**:
The one verified XP Node origin to which the current browser profile directly sends
all console API and status-stream requests. It remains the browser's only direct
control-plane entry while XP performs peer coordination on the server side.
_Avoid_: active node, leader, proxy target

**Backend Candidate**:
A persisted, same-cluster Node origin observed from an authenticated node inventory
and eligible to become the Primary Backend. For a Static Web Console, it must also
be present in its authenticated Console Runtime Policy.
_Avoid_: arbitrary URL, failover URL, endpoint

**Console Origin Allowlist**:
The exact HTTPS Static Web Console origins that an XP Node accepts for cross-origin
console requests. It is distinct from the API origins a browser may call through
`connect-src`; both remain exact, non-wildcard sets.
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
