# Implementation

## Current state

- bootstrap、join、PHC 同步和 leader 版本跟随已实现。
- 管理员凭据只以 PHC 形式落盘；join metadata 保存同一集群 PHC。
- fresh join 的 HTTP 阶段只交付 durable bootstrap material；leader join coordinator 在认证
  runtime 启动后只为 recorded Join operation / JoinSession 执行 catch-up、promotion、重启恢复和
  过期清理。它不会把一般 learner 推测性晋升为 voter。
- 在全 voter `cluster.membership-lifecycle-v1` capability barrier 前，fresh join 冻结并返回
  `coordinated_upgrade_required`。
- host-managed systemd/OpenRC 与 single-image container 继续先运行 `xp join`，再启动 XP runtime。

## Remaining verification

- 三种部署形态必须通过低内存 PHC 升级和回滚测试。
- release candidate 必须在共享 testbox 隔离集群中通过官方 `xp-ops deploy` fresh-node 验证。
