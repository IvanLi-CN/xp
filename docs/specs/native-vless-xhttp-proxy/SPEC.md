# Rust 原生 VLESS + REALITY + XHTTP 代理

## 背景与范围

本主题定义由现有 `xp` 控制面管理的独立 Rust `xp-proxy` 数据面。范围包括与锁定的 Xray/Mihomo Compatibility Baseline 互操作、VLESS 入站与出站、REALITY client/server、XHTTP `stream-one`、XP 现有六项 XMUX 设置，以及固定 HTTP(S) fallback。不定义 XP Raft、Web API 或任意 Xray routing。

## 目标

- 提供独立的 Rust `xp-proxy` Cargo package 和 OS 进程。
- 与 Compatibility Baseline 双向互操作。
- 支持 XP 管理的 VLESS + REALITY + XHTTP endpoint 合同。
- 支持 allowlist canary、HTTP、HTTPS、HTTP/2 和 h2c fallback upstream。
- 保持配置原子性、ready、drain、崩溃恢复和资源边界。

## 非目标

- `xp-proxy` 不直接读取 XP Raft、SQLite 或 Web 状态。
- 不允许任意 fallback URL 或任意 Host 转发。
- 首个生产合同不包含 UDP/XUDP、其他 XHTTP mode 或未经测试的 Xray 字段。
- 不对未锁定的未来 Xray 或 Mihomo 版本作自动兼容承诺。

## 必须满足的合同

### 项目边界

仓库包含独立的 `xp` 和 `xp-proxy` Cargo package。XP 拥有 endpoint/user 权威状态，并生成完整的版本化 Proxy Snapshot。`xp-proxy` 拥有公开 listener、VLESS/REALITY/XHTTP 处理、fallback、连接状态和运行时 metrics。

### 控制协议

两个项目通过受权限保护的 Unix socket 通信。请求和响应包含 `protocol_version`、`request_id` 和 `generation`。最低生命周期为：

```text
idle -> preparing -> validating -> starting -> ready -> active -> draining -> stopped
```

失败 generation 只能进入 `rejected`、`failed` 或 `rollback_required`，不得成为 active。激活前必须完成 listener ready、密码学和配置校验、fallback 校验、本地握手自测，并能够接受新连接。

### 快照语义

Snapshot 是完整且原子的。prepare 或 activate 失败时保留旧 Active Generation。`xp-proxy` 持久化最后一个成功激活的 snapshot；重启或版本冲突时，以 XP 已提交的 generation 为准。

### 协议范围

- VLESS 入站和出站支持 TCP stream 与 `encryption: none`。
- REALITY client/server 支持锁定 Xray 握手、key schedule、short ID、SNI、ALPN、fingerprint、TLS record 处理及 authenticated/unauthenticated 分流。
- XHTTP 支持 HTTP/2 `stream-one` 和 XP 固定 path/mode 合同。
- XMUX 支持 `max-connections`、`max-concurrency`、`c-max-reuse-times`、`h-max-request-times`、`h-max-reusable-secs`、`h-keep-alive-period`。
- fallback 只支持 XP 配置的 canary 和 allowlist HTTP、HTTPS、HTTP/2、h2c upstream，并执行 authority 校验和 hop-by-hop header 清理。

### 兼容性门禁

生产启用必须通过以下四条路径：

```text
Rust inbound  <- Xray outbound
Rust inbound  <- Mihomo client
Rust outbound -> Xray inbound
Rust outbound -> Mihomo-compatible server
```

大包、小包、并发 stream、idle reuse、主动断开、reconnect、错误凭据、畸形握手、fallback 和资源限制均为必测场景。

### 未来 capability

UDP/XUDP、其他 XHTTP mode、高级 padding/session 字段以及额外 Xray 设置，必须先增加显式 capability、兼容 fixture、测试和 SPEC 更新，才能使用。
