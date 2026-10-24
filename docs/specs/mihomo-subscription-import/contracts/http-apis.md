# HTTP API

## Preview Mihomo subscription node import（POST `/api/admin/users/{user_id}/subscription-mihomo-import/preview`）

- 范围（Scope）: internal
- 变更（Change）: New
- 鉴权（Auth）: admin token；沿用现有用户详情与 Profile API 的管理员权限边界

### 请求（Request）

- Headers: Existing admin authentication; request and response MUST use `Cache-Control: no-store` semantics for source-bearing data.
- Query: None.
- Body:

```json
{
  "source": "string",
  "existing_extra_proxies_yaml": "string"
}
```

- `source` is one pasted URL or subscription text; the server performs automatic detection.
- `existing_extra_proxies_yaml` is the current editor draft used for structural validation, duplicate detection, name allocation, and reserved-name checks.
- The source text and fetched body are each limited to 2 MiB; the candidate count is limited to 512.

### 处理（Processing）

- A single trimmed public `http://` or `https://` source is fetched server-side through the existing Mihomo public-only policy.
- Remote fetches use a 15-second total deadline, at most 10 redirects, public-IP resolution on every redirect, and no custom request headers, cookies, Bearer credentials, or proxy settings.
- Non-URL input is detected as Mihomo YAML, Base64 subscription text, or raw URI text in that order.
- Accepted URI schemes are `vless`, `ss`, `vmess`, `trojan`, `hysteria2`, and `tuic` in common standard forms.
- YAML input contributes only its root `proxies` sequence; `proxy-groups`, `rules`, `proxy-providers`, ports, and unrelated fields are ignored and reported.
- The server validates the existing draft as an empty or valid YAML sequence before merging. An invalid draft fails the request without replacement.
- The server deduplicates canonical proxy mappings while ignoring only `name`, skips duplicates already in the supplied draft, allocates deterministic names, rejects system-reserved names, and returns only new accepted nodes.

### 响应（Response）

- Success: `200 application/json`.

```json
{
  "detected_source": "remote_url",
  "added_proxies_yaml": "- name: imported-001\n  type: ss\n  ...\n",
  "counts": {
    "added": 1,
    "skipped": 1,
    "failed": 1,
    "warnings": 1
  },
  "items": [
    {
      "source_index": 1,
      "status": "added",
      "name": "imported-001",
      "scheme": "ss",
      "warnings": [],
      "reason": null
    },
    {
      "source_index": 2,
      "status": "skipped",
      "name": "existing-node",
      "scheme": "vless",
      "warnings": [],
      "reason": "duplicate"
    },
    {
      "source_index": 3,
      "status": "failed",
      "name": null,
      "scheme": "ssr",
      "warnings": [],
      "reason": "unsupported scheme"
    }
  ]
}
```

- `detected_source` is one of `remote_url`, `raw`, `base64`, or `mihomo_yaml`.
- `added_proxies_yaml` is a YAML sequence fragment containing only accepted nodes that are not already represented by the supplied draft.
- `items` MUST not contain complete credentials, subscription tokens, raw URLs, or raw source payloads.
- A successful response with zero added nodes returns an empty `added_proxies_yaml` and a complete report; it does not mutate Profile state.

### 错误（Errors）

- `401 unauthorized`: missing or invalid administrator authentication; retryable: no.
- `404 not_found`: the target user does not exist or is not visible to the administrator; retryable: no.
- `400 invalid_request`: empty source, invalid current draft, malformed YAML, invalid Base64, unsupported required format, or a source that violates input structure; retryable: no.
- `413 request_too_large`: source, fetched body, or candidate count exceeds the documented bound; retryable: no.
- `502 upstream_error`: remote URL DNS, connection, redirect, timeout, non-2xx response, or body decoding failure; retryable: yes after the source is checked.
- Item-level parse failures are returned inside a `200` report when at least one source item can be inspected; they do not become endpoint errors.
- Error messages MUST use safe summaries and MUST NOT echo secrets or the complete source URL.

### 前端合并（Web merge）

- The Web dialog MUST retain a fingerprint of `existing_extra_proxies_yaml` when the request starts.
- The returned fragment MAY be appended only when the current draft still matches that fingerprint; otherwise the preview is stale and the user must run it again.
- Canceling the dialog, closing it, or receiving an error leaves the draft unchanged.
- Adding valid nodes changes only the local `extra_proxies_yaml` draft; the existing Profile save action performs persistence.

### 兼容性与迁移（Compatibility / migration）

- Existing `GET` and `PUT /api/admin/users/{user_id}/subscription-mihomo-profile` contracts remain unchanged.
- The endpoint does not alter `extra_proxy_providers_yaml`, `mixin_yaml`, generated system providers, or proxy-group semantics.
- No source URL, decoded payload, Import Report, background refresh job, or import history is persisted.
