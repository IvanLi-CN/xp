# Internal Auth v2 Contract

## Request

- Reserved headers are `X-XP-Internal-Route`, `X-XP-Cluster-Id`,
  `X-XP-Sender-Id`, `X-XP-Target-Id`, `X-XP-Request-Id`,
  `X-XP-Issued-At` and `X-XP-Internal-Signature`.
- `health-v2` is bodyless `GET /api/admin/_internal/mesh/health`.
- `mesh-v2` is limited to allowlisted internal and Raft routes.
- The canonical request covers version, route, uppercase method, raw URI,
  content type, body length, body SHA-256, cluster, sender, target,
  request ID and issued-at.
- The receiver requires timestamp skew within 120 seconds, a matching cluster
  and target, a current sender member and a valid HMAC.

## Key derivation and acknowledgement

- Derive request and acknowledgement keys with HKDF-SHA256.
- Input key material is parsed CA private-key DER.
- Salt is the CA certificate SHA-256 fingerprint.
- Request and acknowledgement use separate info labels.
- Accepted responses carry `X-XP-Internal-Ack`.
- The ack binds request ID, request digest, issued-at, responder ID and status.
- Response bodies and SSE rely on the trusted TLS termination boundary.

## Authorization

- Only `/raft/*`, `/api/admin/_internal/*` and Mesh health accept v2 auth.
- Normal `/api/admin/*` requires administrator Bearer auth.
- Literal v1 authentication is rejected after cutover.
- There is no nonce header or nonce cache.
