# History

## 2026-07-28

- 将已证明安全的 Tunnel hostname 迁移改为自动执行，避免为共享 Tunnel 切换引入不必要的维护窗口；
  旧 Tunnel 的非 XP ingress 继续原样保留，证明不足仍零写入失败。

- 从 legacy `docs/plan/0013:cloudflare-tunnel-remote-access/` 建立 canonical topic spec。
- 冻结共享 Tunnel 非属配置保护、显式迁移、单进程受控重启和可恢复变更合同。
- 将 Alpine 验证路径、新 Tunnel 预检补偿、服务状态与文件元数据恢复纳入事务边界。
- Legacy 目录保留，等待单独的删除批准。
