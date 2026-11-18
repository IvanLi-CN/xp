# 操作手册

## 安装与来源

权威源码为 `IvanLi-CN/xp` 的 `skills/xp-infra/`。
开发时可使用 checkout；正式运维从同一 GitHub Release 下载版本化 Skill 包。
先核对 Release tag 和源码 SHA，再下载 `xp-infra-skill-vVERSION.tar.gz` 及其 `.sha256`。

```sh
sha256sum --check xp-infra-skill-vVERSION.tar.gz.sha256
tar -tzf xp-infra-skill-vVERSION.tar.gz
tar -xzf xp-infra-skill-vVERSION.tar.gz -C NEW_EMPTY_DIRECTORY
bun NEW_EMPTY_DIRECTORY/xp-infra/scripts/xp-infra --help
```

检查包内 `manifest.json` 的版本、来源仓库、SHA、依赖和文件摘要。
不要覆盖正在使用的旧安装；保留旧目录或符号链接目标以便恢复，再切换 Skill 发现路径。
不得把私人配置复制进安装目录或发布包。原 `agent-toolkit` 副本不是新版本维护来源。

## 检查节点

1. 准备仓库外的 `hosts.txt` 和已明确确认的 `deployment-policy.yaml`。
2. 运行 `policy check`、`inventory`，确认目标 SSH 操作员和系统事实。
3. 运行 `status ALIAS`、`version ALIAS` 和 `port preflight ALIAS`。
4. 运行 `audit ALIAS`，保留真实结果；任何非零退出停止后续修改。

只操作主人明确指定的节点。SSH、DNS、地址、端口或身份不明时不得自动改选目标。
明确区分真实节点证据和测试 fixture，不能用测试通过代替线上检查。

## 升级与复验

独立修改授权和检查全部通过后，host-managed 使用：

```text
sudo xp-ops upgrade --version VERSION
```

该命令没有 `--yes`，不得编造确认参数。
容器使用官方镜像/Compose 替换路径；不把容器运行时作为宿主升级的替代方案。
升级必须保留私人配置、证书、持久数据库及运维人员覆盖值。

升级后重新运行完整审计，并校验实际版本、资产摘要、所选入口进程及真实代理通信。
失败时保留原始证据，按项目正式回滚合同恢复，不随意改模式、端口或拓扑。

## 策略边界

默认要求既有公网入口、Tunnel/DDNS/Geo 和托管端点，SS2022 保持关闭。
例外必须在私人策略中明确记录主人批准和理由，不能为消除审计失败临时放宽策略。
仅将脱敏版本、runtime、验收及失败记录写入私人运维目录。
