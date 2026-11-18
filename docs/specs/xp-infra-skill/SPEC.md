# XP 运维 Skill

## Context and Scope

本主题约束 `skills/xp-infra` 的项目内维护、只读运维审计和发布包。
包含 systemd、OpenRC、官方 Docker/Compose 节点；不改变节点部署方案或授权边界。

## Terms and Interfaces

- `runtime`：节点通过 `xp-ops runtime status --json` 声明的入口程序。
- `xp-infra`：依赖本机 Bun、OpenSSH、curl 的只读 CLI；目标节点不需要 Bun。
- 私人配置：仓库外的 `~/.config/xp-infra/`，允许已有环境变量覆盖。

## Requirements

### REQ-INFRA-SOURCE

XP 仓库必须是 Skill 源码、说明、公开模板和测试的唯一维护来源，保留迁入代码的许可及来源提交。
私人配置、凭据和真实节点清单不得进入源码或发布包。

### REQ-INFRA-RUNTIME

审计和端口预检必须依据节点声明的 active runtime 校验所选程序的服务、进程身份和端口归属。
支持 `xray` 与 `xp-proxy`；未知、失败、切换中、身份变化或观测缺失必须失败。
不得因为另一 runtime 的辅助进程仍在运行而改选入口或跳过失败。

### REQ-INFRA-INGRESS

入口审计必须验证本机及公网的真实 HTTPS 回落，保留证书校验、地址限制和输出边界。
公网入口只允许由所选 runtime 的已验证进程拥有；错误归属或不可达不得报告通过。

### REQ-INFRA-RELEASE

Skill 必须与 XP 使用相同版本和源码 SHA，通过同一 GitHub Release 发布独立压缩包及 SHA256。
包内必须包含完整可执行目录、许可和版本清单；解压后不得依赖外部 toolkit checkout。
安装必须校验来源和摘要，保留旧安装恢复路径，不改写私人配置。

### REQ-INFRA-SAFETY

CLI 和探针必须保持只读，遵守私人策略、非交互权限和确定目标约束。
部署修改必须另有明确授权，并使用官方命令，不得把 fixture 结果替代真实部署健康。

## Verification

### VER-INFRA-SOURCE

- covers: `REQ-INFRA-SOURCE`, `REQ-INFRA-RELEASE`
- 方法：发布包隔离构建、清单及 SHA256 校验、解压后运行 CLI。
- 通过条件：固定来源与版本、目录完整、无外部源码依赖或私人配置。

### VER-INFRA-RUNTIME

- covers: `REQ-INFRA-RUNTIME`, `REQ-INFRA-INGRESS`, `REQ-INFRA-SAFETY`
- 方法：类型化决策测试、Linux 探针测试及现有宿主管理器与容器验证。
- 通过条件：两种 runtime 正常证据通过，错误归属、身份变化和回落失败均拒绝。

### VER-INFRA-LIVE

- covers: `REQ-INFRA-RUNTIME`, `REQ-INFRA-INGRESS`
- 方法：对明确授权的现有节点执行只读完整审计。
- 通过条件：所选 runtime 的入口、真实回落和其余既有策略检查全部通过。

## Related ADRs

None
