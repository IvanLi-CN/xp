# ADR 0037：分离查询窗口校验与库内元数据校验

- Status: accepted

## 背景

历史查询层用同一个区间构造函数同时校验两件语义不同的事：

1. 管理员请求的 query window；
2. 仓库自己持有的 Coverage、Gap 和 watermark。

请求区间需要上限，因为它决定一次查询能碰多少数据。库内事实不需要这个上限，它们是仓库已经持久化的签名结论。

显式普通源前缀恢复（ADR 0034）会写入一条从纪元 0 开始的永久 Gap，并标记
`operator_confirmed_unrecoverable_prefix`。该 Gap 的时间跨度远超两年，于是复用了
请求上限的校验在读取元数据时直接失败。

后果不是"查询变慢"，而是仓库整条查询路径失效：

- 该仓库对任何 query window 都返回 `invalid_request: RangeTooLarge`，包括零长度区间；
- 调用方只能静默丢弃该 peer 的响应，退回到本地五分钟窗口，并报告 `local_only`；
- 管理员面板看到的是 8 秒超时，而不是一个有意义的错误。

同一处设计还掩盖了第二个缺陷。记录分页 SQL 使用 `(?N IS NULL OR column = ?N)` 形式，
而时间边界永远是 `Some`，因此 SQLite 无法把该 OR 转成范围定位，只能扫过整条索引。
5 GB 仓库在缓存释放后单次查询实测 71.6 秒。

## 决定

请求窗口与库内元数据使用不同的区间校验。

- query window 继续受请求上限约束：不超过两年，且 `start <= end`。
- Coverage、Gap、watermark 只要求 `start <= end`。一条从纪元 0 开始的永久 Gap 是合法输入。
- 完整性判定保持不变：它只关心 Gap 是否与 query window 相交，与 Gap 自身跨度无关。

同时收紧两条存储层合同，避免同类性能缺陷复发：

- 记录分页与 Coverage 聚合的谓词必须可下推到索引 seek。禁止 `(?N IS NULL OR ...)`；按实际存在的过滤维度选择字面谓词分支，并为排序键首列给出下界。
- 全仓库 Coverage 使用独立的索引边界 seek，不对历史表做全索引聚合。

响应体与 `complete`/`partial` 判定共用一个谓词，二者不会背离。相交的 gap 上报并计入完整性；
起点为纪元 0 的前缀 gap 无论是否相交都上报并计入，因为它的下界无法证明，end 只是首个保留记录的
观测时间；若按相交过滤，窄窗口会在记录仍缺失时被报成 `complete`。不相交的非前缀 gap 本就不参与
完整性判定，因此去掉它不改变语义。

## 考虑过的备选

- 保留统一校验，改为在写入时截断超限 Gap：拒绝。写入方持有的是已签名证据，改写它等于伪造历史边界。
- 为库内区间放宽上限到固定大值：拒绝。这只是把同一个混淆后的常量调大，任何更早的 Gap 仍会再次触发。
- 让调用方容忍单个 peer 的 400 并继续：拒绝。该 peer 仍无法回答自身持有的事实，`local_only` 会成为常态而非降级。
- 分页改用 `ORDER BY observed_start, observed_end, ...` 以复用既有索引：拒绝。
  对外续页顺序已发布，且 `LIMIT/OFFSET` 实测差距在预算内，不值得为此改合同。

## 后果

- 已持有早期永久 Gap 的仓库重新可用；受影响查询正确报告 `partial`，而不是拒绝。
- `local_only` 回到"全部 ready 仓库都不可用"的原义，不再被元数据校验错误触发。
- 查询成本与 query window 相关，而不是与仓库物理体积相关。subject 分页实测从 878 ms 降到
  10.4 ms，全仓库 Coverage 从 1140 ms 降到 0.11 ms。
- 既有大库首次启动会同步创建新索引，耗时随库体积增长；这段时间不推进 repository worker，也不计为 readiness 失败。
- 摄入侧新增 Observation Span 上界校验，使查询可以安全地用有界放宽下界定位索引。已有超限记录仍按精确 `observed_end` 过滤，结果不变。
- 新增 `(is_tombstone, received_at)` 索引以服务 received-time 边界 seek，换取读路径不再做
  全索引聚合。写放大保持不变，仓库已接近容量边界，因此不采用触发器维护的 singleton 计数。
