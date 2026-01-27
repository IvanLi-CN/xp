# HTTP APIs

## `GET /api/cluster/info`

- Scope: internal
- Change: Modify

### Response (200)

Body (JSON):

- Existing fields (unchanged):
  - `cluster_id` (string)
  - `node_id` (string)
  - `role` (string)
  - `leader_api_base_url` (string)
  - `term` (number)
- New fields:
  - `xp_version` (string, required): xp semver, used by Web onboarding to download matching `xp` and `xp-ops` binaries.

### Compatibility

- Adding `xp_version` is backward-compatible for existing clients.

## `POST /api/cluster/join`

- Scope: internal
- Change: Modify
- Auth: pinned TLS using `cluster_ca_pem` carried by join token (client validates server cert against the pinned CA); request includes join token.

### Request

- Method: `POST`
- Path: `/api/cluster/join`
- Headers:
  - `Content-Type: application/json`
  - `Accept: application/json`
- Body (JSON):
  - `join_token` (string, required): join token（base64url JSON）
  - `node_name` (string, required)
  - `access_host` (string, required)
  - `api_base_url` (string, required): `https://...` origin
  - `csr_pem` (string, required): PEM CSR

### Response (200)

Body (JSON):

- `node_id` (string, required)
- `signed_cert_pem` (string, required)
- `cluster_ca_pem` (string, required)
- `cluster_ca_key_pem` (string, required)
- `xp_admin_token_hash` (string, required): **cluster-wide** admin token hash used to verify `/api/admin/*`

Notes:

- `xp_admin_token_hash` MUST be the leader node's configured hash (the cluster source of truth) so joiners can align without operator input.
- `xp_admin_token_hash` format (v1): Argon2id PHC string (see `contracts/file-formats.md`); legacy `sha256:<hex>` MAY be supported during migration.
- The server MUST NOT log `xp_admin_token_hash` (nor `cluster_ca_key_pem`) in plaintext.
- Security model note: confidentiality relies on TLS pinned by `cluster_ca_pem` carried in the join token; leaking a join token implies the attacker can join and receive the response secrets.

### Errors

- `400 invalid_request`: malformed/expired/used join token; invalid fields; cluster id mismatch
- `401 unauthorized`: reserved (if future auth added; currently join is authorized by join token + pinned TLS)
- `409 conflict`: reserved (if future one-time token collision needs conflict)
- `500 internal`: unexpected errors

### Examples

Request (redacted):

```json
{
  "join_token": "<base64url>",
  "node_name": "node-2",
  "access_host": "hkl.example.com",
  "api_base_url": "https://hkl.example.com",
  "csr_pem": "-----BEGIN CERTIFICATE REQUEST-----\n...\n-----END CERTIFICATE REQUEST-----\n"
}
```

Response (redacted):

```json
{
  "node_id": "01J...ULID",
  "signed_cert_pem": "-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----\n",
  "cluster_ca_pem": "-----BEGIN CERTIFICATE-----\n...\n-----END CERTIFICATE-----\n",
  "cluster_ca_key_pem": "-----BEGIN PRIVATE KEY-----\n...\n-----END PRIVATE KEY-----\n",
  "xp_admin_token_hash": "<redacted>"
}
```
