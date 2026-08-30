# Cloudflare Tunnel 远程访问

## 背景

`xp-ops` 可以在已经由其他服务共享使用的 Cloudflare Tunnel 上配置 XP。此前 provision 会重写
本地 `config.yml` 和远端 ingress，可能移除不属于 XP 的 hostname、规则、注释或设置。

## 目标

- XP 仅拥有本次指定 hostname 的 ingress 规则与其可证明归属的 DNS CNAME。
- 保留共享 Tunnel 的其他 hostname、服务、规则顺序、顶层配置和 DNS 属性。
- 支持单个既有 `cloudflared` 进程；本地配置验证后受控重启该进程。
- 当旧 settings 与目标 Tunnel 不同时，自动迁移已证明归属的 XP hostname；证据不足时零写入失败。
- 所有写入在预检后执行，失败时提供可恢复诊断并补偿已完成的写入。

## 非目标

- 不启动第二个常驻 `cloudflared` 进程。
- 不自动迁移无法证明 zone、DNS、凭据或所有权的资源。
- 不在项目外节点上自动执行生产迁移。

## 行为合同

- 本地 `/etc/cloudflared/config.yml` 使用 `yaml-edit` 定点修改 `tunnel` 与
  `credentials-file`；未命中内容保留原有字节、注释、顺序和排版。原文件没有 `ingress` 时不得
  凭空新增本地 ingress。
- 远端配置先 GET，再保留未知顶层字段、`originRequest` 与其他 hostname 规则；同一 hostname 的
  全部规则被替换为 XP 整站规则。唯一合法 catch-all 必须在最后且原样保留；缺失时补
  `http_status:404`，歧义时零写入失败。
- DNS PATCH 只变更 XP 所有 CNAME 的 `content`，不得重置 TTL、proxied 或其他非 XP 属性。
- 存量 settings Tunnel ID 与请求目标不一致时，XP 自动执行旧 hostname、DNS 和凭据预检；任一
  证明不足时零写入失败。单进程场景仅在旧 Tunnel 只承载 XP hostname 时迁移；共享旧 Tunnel
  必须零写入拒绝，避免其余服务失去 connector。
- `--dry-run` 可以执行只读 Cloudflare GET 并输出影响摘要；不得发出 POST/PUT/PATCH/DELETE、
  写文件或重启服务。
- 本地配置存在 `ingress` 时，修改前必须运行 `cloudflared tunnel ingress validate`；没有本地
  `ingress` 的远程 Tunnel 配置不得运行这个不适用的校验。本地文件原子替换且服务启动/健康检查
  失败时恢复原始文件和服务状态。

## 验收

- Hinet 形状 fixture 在重复 provision 后，外部 hostname、注释、排版及其他键保持不变。
- 远端 fixture 的未知字段、SSH/TCP/path 规则及合法 catch-all 保持不变；XP hostname 的全部规则被替换。
- 自动迁移的任一所有权预检失败不得产生本地、Cloudflare、DNS、settings 或服务变更。
- 每个写入阶段失败时已完成变更按逆序补偿；补偿失败保留快照并输出人工恢复信息。

## 实现里程碑

1. 无损本地编辑和远端 ingress 合并。
2. DNS 所有权判定与显式 Tunnel 迁移预检。
3. 事务执行、受控重启和补偿回滚。
4. CLI/API/失败阶段自动化测试与运维文档同步。

## Legacy

原始材料保留在 `docs/plan/0013:cloudflare-tunnel-remote-access/`。该目录标记为
pending delete approval，未经明确批准不得删除。
