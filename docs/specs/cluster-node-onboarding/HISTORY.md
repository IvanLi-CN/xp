# History

- 2026-01-27: legacy plan 冻结集群共享 PHC、bootstrap 一次显示和 join 同步模型。
- 该主题将原有节点引导计划收敛为 canonical spec。
- 2026-07-29: 低内存参数和生产轮换由 #nmgq8 接管。
- 2026-08-16: fresh join 改为 durable bootstrap 与后台 catch-up/promotion 两阶段协议。
- 2026-09-03: fresh join admission 不再被无关 stale learner 阻塞；legacy 过期 reservation 保留
  learner 与 DesiredState Node，仅记录 session 终结。
