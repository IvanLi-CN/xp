# 完整托管栈 64 MiB 内存预算 (#nmgq8)

## 背景

小型节点需要在 `xp + xray + cloudflared + canary` 全部运行时保持低于 64 MiB
进程 PSS。生产诊断确认两个独立风险：高参数 Argon2 会在每次管理员鉴权时临时
分配 64 MiB；长期运行的 Go 进程会保留已归还堆页。连接数量本身不是已观察到的
主要占用。

## 目标与非目标

- 在 100 条并发 VLESS、持续 50 Mbps、一次管理页并发请求和 24 小时 soak 下，
  总 PSS 峰值不超过 65,536 KiB。
- systemd、OpenRC、单镜像 Docker/Compose 使用相同内存合同。
- 禁止周期或阈值重启掩盖增长；rollout 和故障恢复重启不受此限制。
- 不把内核、page cache 或节点其他进程计入本规格，也不新增通用 metrics 系统。

## 运行时契约

- 管理员 Token 默认 Argon2id profile 为 `m=4096,t=3,p=1`；新明文至少 32 字节。
- 新写入或导入的 PHC 必须匹配该 profile；旧高内存 PHC 只能在迁移前识别，
  不得继续写入。
- JWT 形态凭据先走 JWT 校验，不得先执行 Argon2；原始 Token 校验使用单 worker
  和有界等待，饱和时返回 `429` 与 `Retry-After`。
- Xray 默认 `GOMEMLIMIT=16MiB`、`GOGC=50`，level-0 `bufferSize=0`。
- cloudflared 默认 `GOMEMLIMIT=12MiB`、`GOGC=50`、`--protocol http2`，并关闭管理诊断采集；
  `XP_CLOUDFLARED_PROTOCOL` 可覆盖传输协议；托管服务定义变更后必须 reload 并重启，
  使新传输协议立即生效。
- Release 从固定上游提交以 Go 1.26.5、禁用内联和去符号参数构建 Xray 与
  cloudflared；宿主升级和官方容器镜像必须使用同一组带 checksum 的资产。
- operator 已有显式 override 优先；升级只 backfill 缺失值或 XP 生成的旧默认值。

## 观测与预算

- 每秒从目标进程树的 `/proc/<pid>/smaps_rollup` 读取 `Pss`；旧内核缺失该文件时汇总
  `/proc/<pid>/smaps` 的 `Pss` 条目，记录角色、版本、
  当前值和峰值。
- 诊断子预算为 `xp <=18 MiB`、`xray <=22 MiB`、`cloudflared <=18 MiB`、
  临时 canary `<=4 MiB`；总预算是唯一发布硬门禁。
- 任一 OOM、非预期重启、采样缺口或总峰值超限均失败。

## 部署与回滚

- host-managed unit/service 和容器 supervisor 都必须显式注入对应 Go runtime env。
- host-managed 升级把 Xray 与 cloudflared 视为成对资产；缺失一项、checksum
  失败或服务重启失败时恢复两者和 XP，不允许留下混合版本。
- host-managed 升级必须在可选的 `xp-ops` 自更新前完成锁定版本的 XP 和托管运行时
  阶段；工具二进制更新不得提前结束服务升级。
- systemd/OpenRC 的重启命令返回成功不足以作为升级完成条件；升级必须等待对应
  service manager 报告 XP、Xray 和 cloudflared 已 ready，否则回滚。
- 使用同一低内存 PHC 预写所有节点，再按健康 follower 到 leader 的顺序逐机
  重启或重建。
- 已加入集群的官方 Docker/Compose 节点在宿主 env 显式提供有效 PHC 时，必须在
  启动 XP 前原子同步数据卷中的管理员 hash；未提供时保持既有数据卷状态不变。
  Token 轮换不得依赖 `docker exec` 或容器内的手工元数据编辑。
- 每步验证 quorum、节点可达、Xray、Tunnel 和新 Token；失败即停止后续节点并
  恢复已变更配置。

## 验收

- Rust 与 Web 全量质量门禁通过。
- 三种部署形态覆盖默认值、override、升级 backfill 与回滚测试。
- 共享测试机完成 15 分钟 smoke 和 24 小时生产档 soak。
- 四个生产节点版本和 PHC 指纹一致，旧 Token 失败，新 Token 成功，并完成
  24 小时脱敏 PSS 观察。

## Visual Evidence

PR: none

本规格的变更仅涉及运行时配置、鉴权状态和部署路径，没有可独立验收的界面效果。

## 参考

- `docs/specs/r7m2q-openrc-low-memory-runtime-recovery/SPEC.md`
- `docs/specs/38wmj-cluster-node-onboarding/SPEC.md`
- `docs/specs/k9n2r-xp-ops-admin-token-ops/SPEC.md`
- `docs/solutions/ci/real-xray-e2e-runner-guardrails.md`
