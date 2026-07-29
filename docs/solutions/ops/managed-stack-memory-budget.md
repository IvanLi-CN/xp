# Managed Stack Memory Budget

## Symptom

The XP node inventory can show an unreachable node and `xray`/`cloudflared`
memory growth while the host remains otherwise healthy.

## Root Causes

- The default Xray Go heap is unconstrained and retains buffers after traffic bursts.
- Argon2 verification with the legacy high-memory PHC can allocate tens of MiB
  for each concurrent admin request.
- A JWT already contains a signed, short-lived credential and must not trigger
  an Argon2 verification.

## Remediation

PSS includes mapped executable code as well as heap. If Go heap limits are
active and the aggregate still misses the gate, optimize the Rust release
profile before lowering Go limits into GC churn.

For XP release binaries, use one codegen unit, Fat LTO, `opt-level="z"`,
`panic="abort"`, and symbol stripping. Symbol stripping reduces the artifact,
while size optimization and LTO reduce executable pages that contribute to
PSS. Treat a smaller file as a build signal only; the production PSS sampler
remains the acceptance gate.

Compress text-based embedded Web assets deterministically at build time and
negotiate gzip with `Accept-Encoding`. Serve the compressed representation to
browsers and retain an on-demand decode fallback for clients that do not
advertise gzip. This removes raw JavaScript and CSS payloads from executable
mappings without changing the single-binary deployment contract.

Use Argon2id `m=4096,t=3,p=1` for newly generated administrator PHC values
and require administrator secrets to contain at least 32 bytes. Serialize
raw-token verification behind one bounded worker; return HTTP 429 with
`Retry-After: 1` when the worker is busy. Validate JWT credentials before
considering the raw-token path.

For an existing joined single-image Docker node, treat an explicit host-side
`XP_ADMIN_TOKEN_HASH` as desired state when the container is recreated. Before
starting XP, atomically synchronize that low-memory PHC to the persisted cluster
metadata file. During the first join, preserve the leader-provided hash instead.
Do not rotate a container node through `docker exec` or direct volume edits.

Set Xray `GOMEMLIMIT=16MiB`, `GOGC=50`, and policy level 0 `bufferSize=0`.
Set cloudflared `GOMEMLIMIT=8MiB`, `GOGC=50`, and
`TUNNEL_MANAGEMENT_DIAGNOSTICS=false`. Host upgrades backfill these values
through a managed systemd drop-in or an OpenRC script insertion that leaves
existing operator values untouched. Container launches pass the same values
to child processes and retain explicit `XP_*` overrides.

Some provider networks block outbound UDP/7844. New cloudflared builds begin
with QUIC, and a node can remain unreachable at Cloudflare's edge while its
local process appears healthy.

- Default managed cloudflared to `--protocol http2`.
- Use `XP_CLOUDFLARED_PROTOCOL` only for an explicit operator override.
- Upgrade backfill may update a recognizable managed command, but must leave
  an existing explicit `--protocol` untouched.

Heap tuning alone may not fit a sub-64 MiB process-tree budget because PSS also
charges executable mappings. For the pinned upstream Xray and cloudflared
versions, `go build -gcflags=all=-l -ldflags='-s -w -buildid='` reduced the
combined production canary below the gate without lowering Xray's heap limit
into GC churn. Treat these binaries as release-managed assets: publish both
architectures with checksums, replace Xray and cloudflared as one upgrade set,
and restore both if configuration reconciliation or either service restart
fails.

## Verification

Use `scripts/ops/sample-managed-stack-pss.sh` to sample process-tree PSS from
`/proc/*/smaps_rollup` once per second, falling back to summing `Pss` entries in
`/proc/*/smaps` on older kernels. The hard gate is a combined peak of
`65,536 KiB`; an OOM, restart, or budget breach fails the run.
