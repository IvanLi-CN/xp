# Mihomo 订阅/配置脱敏 CLI（#9fke7）

## 状态

- Status: 已完成
- Created: 2026-03-03
- Last: 2026-03-03

## 背景 / 问题陈述

- 线上排障时经常需要分享 Mihomo 订阅地址、raw/base64 订阅文本与 YAML 配置。
- 当前仓库缺少统一的脱敏入口，容易泄露 subscription token、uuid、password、private/public key 等敏感信息。

## 目标 / 非目标

### Goals

- 在 `xp-ops` 中新增 `mihomo redact` 子命令，支持 URL / 文件 / stdin 输入。
- 默认输出“可读明文”脱敏结果，支持 `raw + base64 + YAML`。
- YAML 脱敏尽量保持原文样式（注释/缩进/顺序不整体重排）。
- 提供仓库脚本别名 `scripts/mihomo-redact.sh`，便于直接调用。

### Non-goals

- 不回写源文件。
- 不支持认证头注入、代理链路等高级下载参数（后续再扩展）。

## 范围（Scope）

### In scope

- `xp-ops` CLI 命令定义、调度与 preflight 放行。
- 脱敏实现模块：输入加载、格式识别、规则化脱敏、stdout 输出。
- 单元测试 + 集成测试（含 URL 拉取）。
- ops 文档与 specs 索引更新。

### Out of scope

- Web UI 增加脱敏入口。
- 修改订阅 API 输出逻辑。

## 需求（Requirements）

### MUST

- 命令形态：`xp-ops mihomo redact [SOURCE]`。
- 输入优先级：`SOURCE`（URL/文件）优先；缺省时读取 stdin。
- 参数：
  - `--level <minimal|credentials|credentials-and-address>`，默认 `credentials`
  - `--source-format <auto|raw|base64|yaml>`，默认 `auto`
  - `--timeout-secs <N>`，默认 `15`
- `auto` 模式需识别 base64 订阅并解码后输出脱敏明文。
- URL 输入执行 HTTP GET 拉取，不附加认证头。
- 脱敏规则覆盖：
  - URI userinfo（uuid/password 等）
  - query 中敏感参数（token/password/uuid/sid/pbk/secret/key...）
  - 订阅 path token（`/api/sub/{token}`、`/sub/{token}`、`/subscribe/{token}`）
  - YAML `key: value` 中敏感字段
- 脱敏策略：保留前后缀（<=8 全 `*`，>8 保留前4后4）。

### SHOULD

- YAML 注释、缩进、键顺序保持不变（仅命中值替换）。
- 对 URL/URI 嵌入值（如 `url:`）应用同一套 URI 脱敏规则。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- `SOURCE` 是 URL：拉取文本 -> 识别格式 -> 脱敏 -> stdout。
- `SOURCE` 是文件：读取文本 -> 识别格式 -> 脱敏 -> stdout。
- 无 `SOURCE`：读取 stdin -> 识别格式 -> 脱敏 -> stdout。

### Edge cases / errors

- 输入为空：`invalid_input`。
- base64 显式模式解码失败：`invalid_input`。
- URL 拉取失败/非 2xx：`network_error`。
- 文件读取失败：`io_error`。

## 验收标准（Acceptance Criteria）

- Given 原始 `vless://...` 订阅行，When `xp-ops mihomo redact`，Then `uuid/pbk/sid` 不再以原文出现，host/port 可读。
- Given 原始 `ss://...` 订阅行，When `xp-ops mihomo redact`，Then method 保留、password 被脱敏。
- Given 订阅 URL 包含 path/query token，When 脱敏，Then token 字段均被替换。
- Given YAML（含注释与缩进），When 脱敏，Then 注释和缩进保留，仅敏感值变化。
- Given `--level credentials-and-address`，When 脱敏，Then server/sni/host 类字段额外被脱敏。
- Given base64 订阅输入，When 脱敏，Then 输出为明文文本且敏感值不暴露。

## 非功能性验收 / 质量门槛（Quality Gates）

- `cargo fmt`
- `cargo clippy -- -D warnings`
- `cargo test`

## 文档更新（Docs to Update）

- `docs/ops/README.md`：新增 `xp-ops mihomo redact` 使用说明。
- `docs/specs/README.md`：新增索引行并维护状态。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: CLI 接口与 preflight 接入（`xp-ops mihomo redact`）
- [x] M2: 脱敏核心实现（输入加载、格式识别、URL/YAML/URI 脱敏规则）
- [x] M3: 测试、脚本别名与文档同步

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：文本保真策略可能无法完美覆盖复杂多行 YAML 标量（best-effort）。
- 开放问题：None。
- 假设：排障场景优先可读性，base64 输入统一输出为明文脱敏文本。

## 变更记录（Change log）

- 2026-03-03: 完成 CLI、脱敏实现、脚本别名、测试与文档同步；提交 PR #92。
