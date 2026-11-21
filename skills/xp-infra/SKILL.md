---
name: xp-infra
description: 管理 XP 节点的部署前检查、升级指导和只读审计，支持 Xray 与 xp-proxy runtime。
---

# XP 运维

本 Skill 的唯一源码是 `IvanLi-CN/xp` 的 `skills/xp-infra/`，与 XP 同版本发布。
本机需要 Bun、OpenSSH 和 curl；目标节点需要 POSIX 工具和支持 runtime status 的 xp-ops。

## 入口

通过本机 Bun 直接运行 [scripts/xp-infra](scripts/xp-infra)。
本机 TypeScript 负责严格解析和判断；远端 POSIX 探针只负责有界的只读观测。

- [CLI 合同](references/cli.md)：命令、依赖、失败语义和边界。
- [操作手册](references/runbook.md)：安装、检查、升级和恢复顺序。
- [上线与排障 FAQ](references/faq.md)：首次上线的常见疑问与判定依据。
- [来源](ORIGIN.md)：迁移来源与许可。

## 配置与授权

操作前读取已确认的私人策略。缺失、不完整或未确认的策略必须阻止操作。
默认配置目录为 `~/.config/xp-infra/`，支持 `XP_INFRA_CONFIG_DIR`、
`XP_HOSTS_FILE`、`XP_POLICY_FILE` 覆盖。SSH 连接资料保留在用户 SSH 配置中。

真实节点别名、地址、域名、凭据、策略与验收记录均不得进入仓库或发布包。
只发布 [公开模板](references/deployment-policy.yaml.example)，不得读取管理员 token。

CLI 和探针只读；部署、升级或配置修改另需主人明确授权，并使用项目官方命令。
`xp-ops upgrade` does not expose `--yes`；不得编造确认参数或改用未授权的手动替换。
SS2022 默认禁用，除非私人策略包含已授权例外。

## 执行顺序

1. 校验策略和已登记 SSH 操作员；不得使用 root 或 xp 账户直接连接。
2. 执行所选节点的 inventory/status/version、端口预检和完整审计。
3. 仅在检查通过且已有修改授权时，执行官方匹配版本的部署或升级命令。
4. 再次完整审计；任何非零结果都必须停止并保留失败，不得记录为通过。
5. 仅把脱敏结果保存在仓库外的私人运维目录。

以节点声明的 active runtime 判断入口程序，支持 `xray` 与 `xp-proxy`。
不得因为另一程序的辅助进程仍在运行而改选入口；未知、切换中、失败或身份变化均拒绝。
进程身份、服务、端口归属及真实 HTTPS 回落必须一致；不得通过改名或跳过检查消除失败。

测试必须区分命令 fixture、隔离 Linux 探针和真实节点观测。
fixture 通过不能证明线上部署健康。
