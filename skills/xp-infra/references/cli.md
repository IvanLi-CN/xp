# CLI 合同

## 运行边界

入口由本机 Bun 直接运行，依赖 OpenSSH 和 curl；不依赖 Ruby 或 Python。
目标节点使用有界 POSIX 探针，不需要 Bun，不安装 helper，不读取管理员 token。

| 命令                             | 行为                                                     |
| -------------------------------- | -------------------------------------------------------- |
| `inventory`                      | 观测已登记节点；任一失败返回非零。                       |
| `policy check`                   | 仅在完整私人策略已确认时通过。                           |
| `port preflight ALIAS`           | 保留已验证的托管入口端口；新节点执行确定性有界端口选择。 |
| `audit ALIAS`                    | 所有必要检查通过才输出 `audit: OK`。                     |
| `status ALIAS` / `version ALIAS` | 有界系统诊断；诊断完成不等于部署成功。                   |
| `runtime ALIAS`                  | 只读比对本地 runtime 选择与开机自启项；不一致返回非零。  |
| `runtime --all`                  | 对清单内所有节点执行同一比对。                           |
| `logs ALIAS --tail N`            | 读取日志，N 为 1–10000，默认 200。                       |
| `tunnel ALIAS [OPTIONS]`         | 前台 SSH 转发，不自动换端口。                            |
| `help`                           | 无需配置或网络的帮助。                                   |

所有参数严格解析，拒绝重复或未知参数；非零结果不得改记为通过。
SSH 操作员必须为已登记的非 root、非 xp 用户。

## Runtime 与归属

`xp-ops runtime status --json` 是 runtime 选择依据。
仅支持 active 的 `xray` 或 `xp-proxy`，未知、失败、切换中及错误状态均失败。
观测前后必须保留同一 runtime、PID/start-time 或容器身份，不得凭进程存在猜测选择。
入口检查使用统一的 `ingress_*` 名称，所选服务及真实公网监听必须一致。
辅助 Xray loopback API 的存在不改变 xp-proxy 入口归属。

`runtime ALIAS` 额外比对开机自启项：systemd 用 `systemctl is-enabled`，OpenRC 用
`rc-update show default`（无需 root）。只有“所选 runtime 已启用且另一个未启用”才算一致，
其余组合返回非零。`/etc/xp` 可能是 `0750 root:xp`，探针改用 `sudo -n xp-ops runtime status --json`
读取，不再直接读 `/etc/xp/runtime.json`。

端口预检要求托管状态、所选进程、服务和监听匹配。
Xray 使用 inbound inventory 校验；xp-proxy 使用持久快照的单一监听和 generation 校验。
多监听或缺失证据必须失败，不得猜选端口。快照仅在内存中解析，不输出凭据或保存私人副本。

## 授权升级

helper 只读；host-managed 节点的正式升级命令为：

```text
sudo xp-ops upgrade --version VERSION
```

`xp-ops upgrade` does not expose `--yes`，不得追加该参数。
必须先通过私人策略、所选节点预检和完整审计，再凭独立修改授权执行官方命令。
Docker/Compose 使用项目正式镜像替换和重启路径，保留现有配置合同。

## 配置与有界输入

私人配置沿用 `XP_INFRA_CONFIG_DIR`、`XP_HOSTS_FILE`、`XP_POLICY_FILE`。
清单只允许唯一的小写 SSH 别名；策略使用 schema 1，拒绝未知键、重复键和 YAML 扩展。
JSON 拒绝重复成员、无效嵌套、不安全数值或不完整数据；记录拒绝重复键及控制字符。

普通命令默认最长 60 秒，SSH 配置查询 15 秒，curl 请求有独立超时。
stdout 和 stderr 独立限制为 65536 字节；超限、无效 UTF-8、信号及非零退出均失败。
远端 stderr 不直接显示，避免泄露私人信息。

公网探测仅允许确定的 HTTPS DNS 目标及固定 health/canary 路径。
校验所有 DNS 地址的公共可达性，固定目标地址，禁用代理，保持 TLS 校验并拒绝重定向。
Tunnel 证明必须同时符合声明、健康响应及 Cloudflare 地址和响应头。
不得因为权限、容器身份或节点观测失败而选择另一节点、端口或 runtime。
