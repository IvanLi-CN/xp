# Serve the Web console as an independent EdgeOne static application

The XP Web console gains an independent, versioned static delivery path at
`https://xp.ivanli.cc`. It is published to EdgeOne Makers from the exact Web
artifact built for an XP release and retained with that release. The static origin
directly calls a manually selected, verified Primary Backend; it is not an API
proxy or a second control-plane service.

The dedicated Makers project is `xp-web`. Its production environment is the only
environment bound to `xp.ivanli.cc`; direct upload is the deployment authority,
not a Makers Git integration. Before the first release, the owner binds the custom
domain, completes ownership verification and CNAME configuration, and, when the
chosen acceleration region includes Chinese mainland, completes the required ICP
filing. GitHub stores the exact project name in `EDGEONE_MAKERS_PROJECT` and the
deployment token only in `EDGEONE_API_TOKEN`; a missing project variable or token
fails the release before a CLI invocation.

Before the first XP release, `xp-web` receives a non-credentialed static placeholder
deployment solely to establish that production binding. The placeholder contains no
Node origin, runtime policy, or administrator data. A normal release replaces it
with the release artifact; this one-time setup prevents a release from publishing
node-upgrade artifacts while `xp.ivanli.cc` is still unbound.

`https://101-xp.ivanli.cc` is the initial Console Bootstrap Origin. A newly opened
or logged-out browser knows only that origin. It authenticates an administrator
there before receiving any Node API origins once that origin is Static
Console-Compatible. A Node becomes Static Console-Compatible only when it admits
the exact Console Origin Allowlist through browser CORS and serves the
authenticated Runtime Policy. Every compatible XP Node derives a Console Runtime
Policy from that Raft-owned allowlist and the current, registered
`Node.api_base_url` set. A policy contains only compatible registered origins, is
authenticated and short-lived, and is the only source of Static Web Console
candidates.

The static document's initial CSP permits only the bootstrap origin. After a
client receives an authenticated Runtime Policy, the Service Worker gives that
client's next navigation the same cached app-shell bytes with an updated exact
`connect-src` header. This is a document reload, not a new Web build or an Edge
Function invocation. A Policy Grant is scoped to one controlled client and is
removed on logout or client loss; static or shared caches never disclose a prior
administrator's topology to an anonymous page.

## Considered Options

- Static CSP released again for every topology change: rejected because Node
  lifecycle would require a separate Web release even when the app-shell code is
  unchanged.
- EdgeOne Function generated CSP on every navigation: rejected because it adds a
  per-navigation function dependency and quota consumption where the authenticated
  XP control plane can supply the policy after login.
- Anonymous runtime-policy endpoint: rejected because it exposes Node topology to
  unauthenticated visitors.
- Browser proxying all APIs through the Service Worker: rejected because it would
  replace the established direct-browser CORS, SSE, mutation-barrier, and backend
  selection contracts.

## Consequences

- This resolves the deferred independent-static-console option in ADR 0011 without
  removing the embedded PWA. Its node-hosted delivery and compatibility behavior
  remain supported during the transition.
- Each Static Console-Compatible Node's browser CORS policy accepts the exact
  static console origin in addition to the existing registered Node origins. CORS
  and the Runtime Policy derive from the same Raft-backed inputs and reject
  wildcards or operator-entered arbitrary origins.
- The app-shell build ID and Runtime Policy ID are independent. A policy-only
  change refreshes the document only when the current client needs an API origin
  absent from its active CSP, its selected origin is removed, or the client must
  recover from an expired policy. Existing clients continue on an already allowed,
  healthy Primary Backend without interruption.
- A Bootstrap Outage fails closed for fresh or logged-out clients. A client with a
  valid Policy Grant can continue against an allowed healthy Primary Backend and
  refresh the policy there. Recovery is an ingress or DNS switch for
  `101-xp.ivanli.cc`, not an app-shell redeploy.
- The release workflow deploys the immutable same-SHA Web artifact to the EdgeOne
  production project before publishing any XP image or GitHub Release asset. The
  build packages `web/dist` with a checksum manifest; the deployment job verifies
  and expands that package, then uses a pinned `edgeone makers deploy` CLI in the
  production environment. A deployment error fails the release and prevents node
  upgrade artifacts from being published. A job retry reuses the same workflow
  artifact; the completed GitHub Release retains that archive and checksum for
  later recovery. Makers' limited deployment history is never the recovery source.
  Pull requests receive no credentialed deploy preview. After a successful upload,
  the job polls `https://xp.ivanli.cc` for at most five minutes and verifies the
  target build declaration, initial bootstrap-only CSP, and non-cacheable `sw.js`.
  A missing or stale custom-domain response fails the release before any node
  upgrade artifact is published. This check uses only public static responses and
  never calls an XP API.
- Static hosting uses only its native SPA fallback and response-header features:
  unknown navigation routes fall back to `index.html`; `index.html` is revalidated,
  `sw.js` is not stored by shared caches, and content-hashed static assets are
  immutable. The initial document CSP has the same non-connection restrictions as
  the embedded console and permits only the Console Bootstrap Origin in
  `connect-src`. No Edge Function, static runtime-policy document, or edge API
  proxy is present.
- The first independent static release is deliberately published before every Node
  upgrade. Until the Bootstrap Origin becomes Static Console-Compatible, it
  presents only its local compatibility-pending state; it does not discover,
  enumerate, or probe legacy Nodes that will reject the static origin at CORS. The
  embedded node-hosted PWA remains the operational transition path. Once the
  Bootstrap Origin is upgraded, it issues the authenticated policy and later
  upgraded Nodes join that policy as candidates. This is a one-time capability
  migration, not a requirement to wait for Node upgrades on later Web-only
  releases.
- Static Web rollback is a separate protected manual workflow. It accepts only a
  release archive whose checksum and declared Web/API compatibility window remain
  valid, deploys that archive to `xp-web`, and repeats the production-domain
  verification. It neither creates a tag nor modifies images or Nodes; an
  arbitrary historical tag is not an emergency rollback source.
- Anonymous health-response hardening is a separate security decision. The static
  console must not depend on anonymous health diagnostics for bootstrap or policy
  discovery.
