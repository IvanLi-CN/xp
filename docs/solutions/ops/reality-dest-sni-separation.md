# VLESS REALITY destination and SNI are separate on manual endpoints

## Symptoms

- A REALITY endpoint is reachable when Xray dials one camouflage destination, but clients must use a different SNI hostname from the same provider.
- Updating `server_names` through the admin API unexpectedly changes `reality.dest` to the first SNI hostname.
- The endpoint remains unavailable after changing the SNI list because the destination origin was changed at the same time.
- A managed-default VLESS endpoint exposes custom SNI/dest controls even though xp later rewrites them to the canary contract.

## Root cause

XP historically treated `server_names[0]` as the single primary hostname for both client SNI and Xray `dest`. That is only valid when the destination origin and the client-facing SNI are the same host.

Some REALITY camouflage targets require a split configuration. For example, `oneclient.sfx.ms:443` can be the destination origin while `public.sn.files.1drv.com` and `public.bn.files.1drv.com` are better client SNI candidates.

## Fix used here

Preserve explicit `reality.dest` for manual VLESS REALITY endpoints. In manual mode, `server_names` is only normalized and validated as the client SNI candidate list.

Keep the derived behavior for global domain mode. When `server_names_source=global`, XP still derives both the SNI list and `dest=<first global server name>:443` from the global REALITY domain registry.

For managed-default VLESS endpoints, do not expose SNI/dest as operator choices. The managed contract fixes `reality.dest` to `XP_VLESS_CANARY_BIND` and fixes `server_names` to `[node.access_host]` without a port. Non-probe HTTPS canary traffic is handled by endpoint-level `canary_upstream`, routed by HTTP `Host` / `:authority`. The canonical authority remains `node.access_host[:endpoint_port]`, while `accepted_authorities` provides an unordered extra `host[:port]` alias set for ordinary HTTPS camouflage only; omitted port means HTTPS default `443`.

## Verification

- Add state-machine coverage proving manual endpoints keep their explicit `dest` when `server_names` changes.
- Keep state-machine coverage proving global endpoints still derive `dest` from the first global hostname.
- Cover managed defaults proving `reality.dest` is derived from `XP_VLESS_CANARY_BIND` while SNI is derived from node `access_host`.
- Cover canary routing proving canonical and alias `Host` / `:authority` values map to a unique endpoint authority, and that public misses collapse to a plain text `404 Not Found` surface.
- Before changing production, test the exact destination/SNI pair from an external node; do not infer that a successful SNI also works as the destination origin.
