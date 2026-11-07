# ADR 0031：历史仓库 payload 的无损本地编码

## 状态

已接受。

## 背景

仓库同时保存 materialized record 和可重放的 signed segment。JSON wrapper 将字节数组再次展开成数字，造成落盘放大。

抽样中，20 条 runtime 的 506,170 字节语义载荷对应 1,716,725 字节 wrapper。减少保留期会丢数据，修改签名线会破坏验证；加速分页不能消除放大。

## 决定

SQLite 存储边界使用带版本、原始长度及 Zstandard 校验和的本地 envelope。小于 512 字节、超过 16 MiB 或无收益的 payload 保留原字节。

压缩复用一个互斥保护的 bulk context，窗口限制为 64 KiB，避免后台线程反复分配 streaming workspace。
Linux/glibc 启动时在 Tokio worker 创建前默认限制两个 allocator arena，显式 `MALLOC_ARENA_MAX` 优先。
musl 与其他 allocator 保持原行为；该限制针对并发任务释放 payload 后仍由 glibc 保留的堆空间。

每行仍是独立的带校验和 frame，不共享字典或解码状态。

所有行读取和 v3 summary 的直接 payload 读取先解码，再执行既有处理。canonical 字节、签名、摘要和协议保持一致；该格式不依赖 SQLite 扩展。

新增写入直接编码。旧行通过 rowid keyset 分页改写，每表的 payload 更新与游标原子提交，重启后续传。每表每页最多 128 行、累计 1 MiB；较大的旧行独占一页。

后台每五秒最多处理八页、共享两秒预算，页间释放 replica 锁。两表分别提交；下一表失败不撤销前一表已完成的提交。正常读取支持混合编码，损坏 envelope 返回存储错误。

迁移沿用数据库和增量 vacuum，单次最多释放 512 页，不运行 full VACUUM、手动 checkpoint 或建库替换。
必须耗尽 pragma 返回行才能执行完整页预算；普通提交最多 64 页，零预算不执行，避免 SQLite 将零解释为无限制。

物理编码迁移不增加来源交付计数，不改 segment identity 或 revision。record 更新触发既有 dirty-block 重算，摘要必须与迁移前相同。

## 后果

压缩增加有界 CPU 开销，须验证混合读取、重启、失败回滚、摘要一致和损坏输入。通过 durable cursor、日志和物理文件变化观察进度。

旧构建缺少解码器，不能读取已迁移仓库。故障应由具备解码器的新构建前向修复，不得降级。正式发布的状态兼容性按 minor 变更验证。
