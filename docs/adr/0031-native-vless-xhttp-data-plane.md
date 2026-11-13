# Rust 原生 VLESS XHTTP 数据面

状态：accepted

XP 将继续负责控制面，并新增独立的 Rust `xp-proxy` 项目负责 VLESS + REALITY + XHTTP 数据面。XP 拥有 Raft 状态、用户、endpoint 和版本化 Proxy Snapshot；`xp-proxy` 拥有 listener、协议处理、fallback 流量和运行时连接状态。两个项目通过受权限保护的 Unix socket 和版本化快照/生命周期协议通信。首个生产合同锁定 Xray/Mihomo Compatibility Baseline，支持 TCP-only VLESS、XHTTP `stream-one`、现有六项 XMUX 设置以及 XP 固定的 allowlist HTTP(S) fallback。这样可以隔离数据面故障和升级影响，同时把互操作性变成可执行的测试合同。

## 考虑过的方案

- 将原生协议实现嵌入现有 `xp` 进程。放弃，因为这会把数据面故障、资源限制和升级状态耦合到控制面。
- 在现有 `xp` Cargo package 中增加另一个 binary。放弃，因为代理是拥有独立依赖、发布和运行边界的第二个项目。
- 继续使用 Xray 作为数据面。迁移期间保留，但不满足 Rust 原生实现目标。

## 后果

- 仓库改为 Cargo workspace，并包含两个独立 package。
- XP 需要实现快照发布、ready、drain 和恢复协调。
- `xp-proxy` 必须通过锁定的 Xray/Mihomo 互操作矩阵后才能用于生产。
- 未来的 XHTTP mode、UDP/XUDP 或其他 Xray 字段必须通过显式 capability 和新的兼容性证据。
