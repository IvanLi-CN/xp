# HTTP APIs (spec #yzqn6)

## Removed APIs

The following admin APIs are removed and must return `404 not_found` after rollout:

- `GET /api/admin/grant-groups`
- `POST /api/admin/grant-groups`
- `GET /api/admin/grant-groups/:group_name`
- `PUT /api/admin/grant-groups/:group_name`
- `DELETE /api/admin/grant-groups/:group_name`

## New APIs

### GET `/api/admin/users/:user_id/grants`

Response `200`:

```json
{
  "items": [
    {
      "grant_id": "grant_...",
      "user_id": "user_...",
      "endpoint_id": "endpoint_...",
      "enabled": true,
      "quota_limit_bytes": 0,
      "note": null,
      "credentials": {
        "vless": {
          "uuid": "...",
          "email": "grant:..."
        }
      }
    }
  ]
}
```

Behavior:

- Returns only effective grants (`enabled=true`) for the user.
- Does not expose or depend on `group_name`.
- `404` when user does not exist.

### PUT `/api/admin/users/:user_id/grants`

Request:

```json
{
  "items": [
    {
      "endpoint_id": "endpoint_...",
      "enabled": true,
      "quota_limit_bytes": 0,
      "note": null
    }
  ]
}
```

Response `200`: same shape as GET.

Behavior:

- Hard cut replace.
- After apply, effective grants for the user are exactly `items` (dedup by endpoint_id).
- `items=[]` is allowed and means clear all effective grants.
- Existing credentials/grant IDs are reused when endpoint membership remains unchanged.
- `404` when user/endpoint not found.

## Compatibility

- Non-admin subscription APIs are unchanged in format.
- Existing persisted `group_name` data is consumed by migration and removed from runtime model.
