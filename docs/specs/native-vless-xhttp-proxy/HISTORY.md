# 主题历史

- 2026-11-24：出站拨号改为 250 ms 错峰双栈并发（Happy Eyeballs 语义），取代按解析顺序串行尝试。
  现场故障为 us2 实例残留 `default via fe80::1 dev eth1` 的 IPv6 黑洞路由：AAAA 连接挂起使所有域名代理
  目标超时，而 canary/fallback 仍 204、审计全绿。四个域名拨号点（raw fallback、TLS fallback、VLESS 目标、
  XHTTP `stream-one`）统一走 `dial::connect_tcp`/`connect_authority`；不新增自有 DNS。
- 2026-11-24：新建 host-managed 部署默认选择 `xp-proxy`，既有节点保留原 runtime，缺失状态仍按
  legacy Xray；新增 `xp-ops deploy --runtime` 与 `/etc/xp-ops/deploy/runtime-selection.json`
  重试记录，deploy/switch/upgrade 统一收敛 systemd/OpenRC boot enablement，失败 fail closed。
  容器仍为 Xray-only。

- 对齐宿主单节点单模式合同：显式节点 runtime 切换接管本机受支持的 XHTTP 端点，不要求逐端点 owner 迁移。
  HTTPS fallback 修复与服务验收见 [PR #493][fallback-pr]。候选 `ca844c86` 的 [GitHub Actions][fallback-ci] 在
  systemd/OpenRC 下完成 Xray、xp-proxy 重启、升级及坏运行时回滚，恢复二进制和 HTTPS/H2
  204；探针验证每次操作完成后的监听独占，不能作为全转换窗口持续采样的证明。

[fallback-pr]: https://github.com/IvanLi-CN/xp/pull/493
[fallback-ci]: https://github.com/IvanLi-CN/xp/actions/runs/44379309936/job/135066885602

- 2026-11-17：修复出站连接池在容量检查与通知首次轮询之间的丢失唤醒，加入双等待者确定性交错回归；自动适用性补齐实际使用的 Mihomo 安装 fixture。

- 2026-11-17：复测实际主干后补齐 NODELAY 和专用出站任务归属。Linux 单变量对照确认小包停顿；本地重置回归先失败再通过，保留半关闭响应。

- 2026-11-17：建立 TLS 接收积压阻塞反向发送的确定性回归；应用 record 改为直接 I/O，移除后台 duplex 桥接。默认线程数、密钥算法和内存配置不变。

- 2026-11-17：收敛 xp-proxy 性能修复与 Actions 需求：轻量三分钟、全量六分钟设计预算，复用独立上游构建产物，候选/main/Xray 同场对照；PR label 仅豁免合
  并门禁，测量与 main push 测试持续执行。

人工按 run/attempt 作废或恢复结果，独立 Check 展示并验证指定操作工作流来源，证据按九十天留存设计。本轮仅修改需求文档，不表示性能修复、工作流或验收已完成。

- 2026-11-17：将并发准入、响应窗口归还、首包解析和取消分片修复提取为最小正确性依赖，建立可用主干基线；补齐上传结束后取消静默响应的许可释放回归。性能优化和验收工作流保留在独立 PR。

- 2026-11-14：完成独立数据面设计审查。锁定 XP 控制面与 `xp-proxy` 独立 service、显式
  endpoint owner、单端口唯一 owner、持久 generation、fail-closed 重启、systemd/OpenRC
  生命周期和 Xray/Mihomo 四路径证据门禁。记录 service identity、generation 来源、fallback
  语义、drain SLA、自动 rollback、capability 命名和 Docker 首发范围为主人确认项；未授权
  实现、部署或切换 us3。
- 2026-11-05：确定 native proxy 为第二个独立 Rust 项目。首个生产范围固定为 TCP VLESS、
  REALITY、XHTTP `stream-one`、六项 XMUX 设置和 allowlist HTTP(S) fallback；UDP/XUDP 与
  其他 XHTTP mode 延后到显式 capability 和兼容性证据之后。

## IPv6 TCP 支持

将 XP 的 IPv4 wildcard 解释为自动双栈 wildcard，补齐原始 VLESS IPv6 目标拨号和固定 fallback 的 IPv6 authority 验证。

保留独立进程、control protocol v2、显式 runtime switch 和唯一端口 owner 合同；本任务不执行生产发布或节点网络变更。

PR #492 的并发审查发现控制响应写入持有全局转换锁；修复为先取得响应快照、再释放锁执行 I/O，并通过响应背压下的独立 Stop 回归。
