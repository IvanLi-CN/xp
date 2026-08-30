# Implementation

## Cloudflare ownership boundary

- `src/ops/cloudflare_config.rs` 承载本地 YAML 定点编辑与远端 ingress 合并的
  纯函数。
- `xp-ops cloudflare provision` 和 `xp-ops deploy` 自动迁移已证明归属的 XP hostname；
  `--migrate-existing-tunnel` 仅保留为兼容选项。
- Cloudflare DNS 更新使用最小 PATCH，只更新目标 CNAME 的 Tunnel 内容。

## Runtime safety

- 本地 Cloudflared 配置含有 `ingress` 时，写入前用当前发行版安装路径运行 ingress validation，
  使用同目录临时文件和原子替换；远程 Tunnel 形式的无本地 `ingress` 配置跳过该本地校验。
- host-managed 服务在配置变化后重启并检查既有 `cloudflared` 服务；失败时恢复原 enable/running
  状态及受影响文件的内容、权限和属主，不会创建第二个常驻进程。
- 新 Tunnel 的后续远端配置或 DNS 预检失败会立即删除刚创建的 Tunnel，避免留下未受管资源。

## Validation

- Rust 单元与 API smoke 测试覆盖本地无损编辑、远端规则合并、catch-all 歧义拒绝和
  Tunnel/DNS 预检；事务失败路径保留本地/远端快照并逆序补偿。
- `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test` 与
  `bun run check:style-budget`。
