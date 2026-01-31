# HTTP APIs

## Version check（GET /api/version/check）

- 范围（Scope）: internal
- 变更（Change）: New
- 鉴权（Auth）: none

### 请求（Request）

- Headers:
  - `Accept: application/json`
- Query: None (v1)
- Body: None

### 响应（Response）

- Success: `200 application/json`

```json
{
  "current": {
    "package": "0.0.0",
    "release_tag": "v0.0.0"
  },
  "latest": {
    "release_tag": "v0.0.0",
    "published_at": "2026-01-31T00:00:00Z"
  },
  "has_update": false,
  "checked_at": "2026-01-31T00:00:00Z",
  "compare_reason": "semver",
  "source": {
    "kind": "github-releases",
    "repo": "IvanLi-CN/xp",
    "api_base": "https://api.github.com",
    "channel": "stable"
  }
}
```

字段说明：

- `current.package`: 当前运行版本号（优先来自 `XP_BUILD_VERSION`；否则回退到 `CARGO_PKG_VERSION`）
- `current.release_tag`: 当前版本对应 tag（默认映射为 `v{current.package}`；是否需要独立注入由实现阶段决定）
- `latest.release_tag`: GitHub 最新 stable release 的 tag（例如 `v0.2.0`）
- `latest.published_at`: GitHub release `published_at`（可选，展示用）
- `has_update`: `true/false/null`（`null` 表示无法比较，例如版本非 semver）
- `checked_at`: 本次结果生成时间（RFC3339）
- `compare_reason`: `semver|uncomparable`（或等价枚举）
- `source`: 上游解析信息（用于 UI 展示与排障）

### 错误（Errors）

- 上游不可用/速率限制/解析失败：
  - `5xx`（建议对齐 `pod-upgrade-trigger`：让 UI 走 error 分支与重试）
  - （若选用 `200` + `has_update=null` + `compare_reason=...` 的降级语义，必须在实现前置条件冻结）

### 示例（Examples）

- Up-to-date:

```json
{
  "current": { "package": "0.1.0", "release_tag": "v0.1.0" },
  "latest": { "release_tag": "v0.1.0", "published_at": "2026-01-31T00:00:00Z" },
  "has_update": false,
  "checked_at": "2026-01-31T00:00:00Z",
  "compare_reason": "semver",
  "source": { "kind": "github-releases", "repo": "IvanLi-CN/xp", "api_base": "https://api.github.com", "channel": "stable" }
}
```

- Update available:

```json
{
  "current": { "package": "0.1.0", "release_tag": "v0.1.0" },
  "latest": { "release_tag": "v0.2.0", "published_at": "2026-01-31T00:00:00Z" },
  "has_update": true,
  "checked_at": "2026-01-31T00:00:00Z",
  "compare_reason": "semver",
  "source": { "kind": "github-releases", "repo": "IvanLi-CN/xp", "api_base": "https://api.github.com", "channel": "stable" }
}
```

### 兼容性与迁移（Compatibility / migration）

- v1 仅支持 stable channel（GitHub `releases/latest`）。
- 如需 prerelease，必须通过新增 query（例如 `?channel=prerelease`）或新增 endpoint 明确引入，不复用/改变既有字段语义。

### Upstream notes

- 上游数据来源为 GitHub Releases（stable：`releases/latest`）。这属于实现细节，不作为本项目对外契约；但必须实现缓存/节流以避免触发 rate limit。
