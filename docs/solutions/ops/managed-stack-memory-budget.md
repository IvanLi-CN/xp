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

Set Xray `GOMEMLIMIT=16MiB`, `GOGC=50`, and policy level 0 `bufferSize=0`.
Set cloudflared `GOMEMLIMIT=12MiB`, `GOGC=50`. Host upgrades backfill these
values through a managed systemd drop-in or an OpenRC script insertion that
leaves existing operator values untouched. Container launches pass the same
values to child processes and retain explicit `XP_*` overrides.

## Verification

Use `scripts/ops/sample-managed-stack-pss.sh` to sample process-tree PSS from
`/proc/*/smaps_rollup` once per second, falling back to summing `Pss` entries in
`/proc/*/smaps` on older kernels. The hard gate is a combined peak of
`65,536 KiB`; an OOM, restart, or budget breach fails the run.
