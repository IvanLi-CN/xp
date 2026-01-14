# HTTP APIs Contracts（#0012）

本计划默认复用既有 Admin APIs；如实现阶段需要更高效的拉取/保存，也在此文件冻结可选扩展接口。

## Auth（统一）

所有 Admin APIs 使用：

- Header: `Authorization: Bearer <adminToken>`
- Response: `application/json`

错误返回口径沿用现有 `backendError` 约定（status/code/message）。

## Existing (depended) APIs (Change: None)

### List nodes

- Method: `GET`
- Path: `/api/admin/nodes`
- Response (success): `{ items: Array<{ node_id: string; node_name: string; api_base_url: string; public_domain: string }> }`

### List endpoints

- Method: `GET`
- Path: `/api/admin/endpoints`
- Response (success): `{ items: Array<{ endpoint_id: string; node_id: string; kind: string; port: number; tag: string; meta: object }> }`

### List grants

- Method: `GET`
- Path: `/api/admin/grants`
- Response (success): `{ items: Array<{ grant_id: string; user_id: string; endpoint_id: string; enabled: boolean; quota_limit_bytes: number; cycle_policy: string; cycle_day_of_month: number | null; note: string | null; credentials: object }> }`

### Create grant

- Method: `POST`
- Path: `/api/admin/grants`
- Request:
  - Body: `{ user_id: string; endpoint_id: string; quota_limit_bytes: number; cycle_policy: string; cycle_day_of_month: number | null; note?: string | null }`
- Response (success): `AdminGrant`

### Delete grant

- Method: `DELETE`
- Path: `/api/admin/grants/{grant_id}`
- Response (success): `204 No Content`

## Optional extensions

### Filter grants by user (Change: Modify)

目的：矩阵编辑通常只需要某个 user 的 grants；避免拉取全量后在前端过滤。

- Method: `GET`
- Path: `/api/admin/grants`
- Query:
  - `user_id?: string`
- Response (success): same as existing list grants
- Notes:
  - `user_id` 缺省时与当前行为一致（返回全量）

### Batch apply matrix changes (Change: New)

目的：一次提交多个 create/delete，减少多请求往返，并可实现原子/半原子策略。

- Method: `POST`
- Path: `/api/admin/grants/batch`
- Request:
  - Body:
    - `user_id: string`
    - `adds: Array<{ endpoint_id: string; quota_limit_bytes: number; cycle_policy: string; cycle_day_of_month: number | null; note?: string | null }>`
    - `deletes: Array<{ grant_id: string }>`
- Response (success):
  - `{ created: AdminGrant[]; deleted: string[] }`
- Errors:
  - 对于局部失败策略（是否允许 partial success）需要在实现阶段定案；未定前默认要求“要么全成功要么全失败”（atomic best-effort）。
