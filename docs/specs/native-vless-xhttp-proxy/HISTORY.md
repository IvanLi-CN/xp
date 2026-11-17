# 主题历史

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
