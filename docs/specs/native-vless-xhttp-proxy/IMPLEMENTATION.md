# Rust 原生 VLESS + REALITY + XHTTP 实现计划

## 计划任务

1. 将仓库转换为 Cargo workspace，同时保留现有 `xp` package。
2. 新增 `crates/xp-proxy-core` 存放协议代码，新增 `crates/xp-proxy` 存放可执行程序。
3. 锁定并记录 Xray/Mihomo Compatibility Baseline 和 golden configuration fixture。
4. 实现 VLESS framing、用户认证、TCP forwarding 和流量计量。
5. 实现 REALITY client/server 握手和 TLS record 集成。
6. 实现 XHTTP `stream-one`、HTTP/2 flow control、取消和 body streaming。
7. 实现六项 XMUX 设置及连接池生命周期。
8. 实现固定 canary 和 allowlist HTTP(S)/H2/h2c fallback。
9. 实现 Unix socket snapshot/control 协议和 generation 状态机。
10. 增加 Rust、Xray、Mihomo、fallback、fuzz、资源和部署测试。
11. 接入 XP 的 snapshot 发布、ready、drain、恢复和监控。
12. 只有完整兼容性和部署门禁通过后，才能启用生产数据面。

## 所有权

`xp-proxy-core` 拥有协议语义；`xp-proxy` 拥有进程生命周期和本地 control socket；`xp` 拥有集群权威、snapshot 生成、部署协调和用户/endpoint 状态。
