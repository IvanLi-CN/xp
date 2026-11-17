# xp-proxy 性能验证

长期合同见 [性能合同](../../../docs/specs/native-vless-xhttp-proxy/contracts/performance.md)。

`xp-proxy-perf` 对相关 PR/main 变更自动运行轻量矩阵；手动入口选择 `full` 运行全量。
新工作流首次合并前，通过既有 `ci-performance` 手动入口的 `proxy_profile=full` 验证完整链路；
该选项直接调用相同 proxy workflow，并跳过原 CI 构建性能矩阵。默认 `none` 保持原行为。

构建由独立 reusable workflow 提供；消费 job 不编译，下载准确 SHA 的候选与 main 产物。
候选和 main 使用相同 release profile，三程序在同一 runner 串行执行两次反向 sweep。
`main` 在请求建立时锁定，新请求重新解析，不选旧 release 补位。

压测工具 `perf_harness` 是 example，不是生产入口。每条连接持续发送有序帧：操作、序号、长度、
业务数据及 SHA256 ACK。延迟负载为 1 KiB，上传/下载使用持续 64 KiB 帧，每场景至少确认 1 MiB 实际数据。
独立互操作测试另对每个客户端验证单次 1 MiB 大帧，避免用短槽截止替代功能完整性检查。
槽内 1.8 秒发新事务、最多 0.2 秒收尾；超时单列且不能计入确认吞吐。每场景每 sweep 的延迟样本须达到 100。
持续采样结束时，源站确认最后一笔事务及终止帧后，客户端主动取消采样 socket，并明确记录
`planned_cancel_after_terminal_ack`；未确认事务仍算失败。Xray 的正常 TCP half-close 策略会保留
连接数秒，不能将这段策略等待误作传输超时。独立功能联测仍使用 graceful EOF 验证完整收尾。
TLS 伪装源站使用 Python/OpenSSL 并发握手；锁定 REALITY 会镜像单条握手 record 长度，不能使用
合并握手 record 的 rustls 源站，也不能使用串行接受连接的 `openssl s_server`。
锁定 REALITY 的冷启动可能等待目标站 post-handshake 长度缓存初始化五秒；目标源站采用
15 秒握手期限，正式采样前以单次完整事务做最多 10 秒的有界预热，并记录预热耗时和失败。
这段准备计入消费墙钟，不改变正式两秒槽；预热失败保留为非通过。
生产 H2 使用有界的 256 KiB stream / 1 MiB connection 接收窗口；窗口信用不等于预分配内存，
仍逐 chunk 释放容量，实际 PSS 保留在报告中。
`crypto_capacity` 单独记录实际 TLS record 后端的 16 KiB 加密/解密容量，辅助定位瓶颈。
客户端、代理、源站是独立进程，但共享 runner CPU；报告不能宣称独占物理资源或长期稳定性。

`measurement.json` 保存全部原始矩阵、直连对照、50 ms CPU/PSS/FD 采样及逐场景回收检查。
`summary.md` 显示两次 sweep，不通过平均掩盖一次退化。
直连吞吐不足被测值的 120% 时报告容量余量不足；采样、基线或资源证据缺失均不通过。
CPU/PSS 只报告，不设新增数值门槛。主干自身有缺陷时仍保留失败，不自动判断豁免。

完整消费墙钟包括 checkout、下载、验证、运行和上传。上传前计时写入 artifact，包含上传的最终计时
写入同一个 job 的最后日志和 summary。预算为轻量 180 秒、全量 360 秒，安全超时为 300/600 秒。
前者超出告警，后者终止；上游构建和排队不计入消费墙钟。

`xp-proxy-perf-gate` 读取当前 PR 的 `perf:waive`，只改变合并门禁。
加标/去标只重评门禁，不重新构建；实际测量继续，main 不继承标签。
不要把 `measurement` 再设为必要检查，否则会抵消人工豁免。工作流不修改仓库分支规则。

默认分支的 `xp-proxy-perf-result` 接受准确 run ID、attempt、`invalidate`/`restore` 和人工理由。
先上传不可变 receipt，再投影独立的 `XP performance result disposition` Check；原始 run 不被改写。
同一目标串行追加操作，重新运行同一个操作仅修复投影。Checks 只是展示，读取端验证实际手动工作流
及 receipt 来源、内容摘要、前驱和有效期；同名 Check 不能证明有效性。
标记只影响该 run/attempt；同 SHA 的新 run 仍可测试。原始失败不会因 restore 变为通过。
审计/结果统一留存 90 天，受仓库上限约束；到期或审计读取失败不可复用，不自动恢复旧结果。

本地确定性验证：

```sh
cargo test -p xp-proxy -p xp-proxy-core
cargo clippy -p xp-proxy -p xp-proxy-core --all-targets --all-features -- -D warnings
python3 -m unittest discover -s scripts/perf/xp-proxy -p 'test_*.py' -v
actionlint .github/workflows/xp-proxy-perf*.yml
```

真实跨实现回归使用相同上游二进制。`interop` job 先完成 Xray 双向 1/8/64 并发、三类负载，
成功后才安装和运行 Mihomo。它与性能判定独立，不以互操作成功替代吞吐及延迟门槛。
