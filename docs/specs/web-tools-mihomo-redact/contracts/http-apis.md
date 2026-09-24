# HTTP APIs

## POST `/api/admin/tools/mihomo/redact`

Admin-only.

Request body:

```json
{
  "source_kind": "text",
  "source": "string",
  "level": "credentials",
  "source_format": "auto"
}
```

Fields:

- `source_kind`: `text | url`
- `source`: input text or source URL
- `level`: `minimal | credentials | credentials_and_address`
- `source_format`: `auto | raw | base64 | yaml`

Response `200`:

```json
{
  "redacted_text": "string"
}
```

Behavior:

- `source_kind=text`: treat `source` as the raw input body and run the shared Mihomo redaction pipeline.
- `source_kind=url`: allow only public `http/https` URLs, resolve the target host before fetch, reject any loopback/private/link-local/documentation/reserved target, then fetch with a fixed `15s` timeout and no extra headers.
- `source_format=auto`: if the input is a base64-encoded subscription payload, decode it first and return redacted cleartext.

Errors:

- `401 unauthorized`: missing or invalid admin token
- `400 invalid_request`: empty source, unsupported scheme, invalid base64, or URL resolving to a non-public target
- `502 upstream_error`: remote URL fetch failed or returned non-2xx
