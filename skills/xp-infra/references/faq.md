# 上线与排障 FAQ

面向“把一台新机器接入 XP 集群”的常见问题。答案只写可复用事实与命令形态；
别名、地址、域名、端口、凭据留在仓库外。

分两类，阅读时先分清：

- **只读**：观测、审计、preflight，可随时执行。
- **需授权**：部署、升级、runtime 切换、成员驱逐，必须已有主人明确授权。

## 首次上线

### 上线前至少确认哪些事实？

- 私人策略已确认（`xp-infra policy check` 通过），清单里只有小写 SSH 别名。
- SSH 操作员已登记，且不是 `root`、`xp`。
- 资源下限：`docs/ops/README.md` 的 Low-memory host defaults 覆盖 `256 MB` 无 swap 场景；
  升级前两个写文件系统各需 ≥ 128 MiB 可用；`free -m` 要有真实余量。
- 目标节点身份唯一：名称、`access_host`、管理地址都不与现有成员冲突。

### 先跑什么，后跑什么？

先只读观测，再动配置：

```sh
xp-infra inventory
xp-infra status ALIAS
xp-infra version ALIAS
xp-infra port preflight ALIAS
xp-infra audit ALIAS
```

`audit` 通过才进入部署；任何非零结果都停下保留证据，不要把失败改记为通过。
`inventory` 只要有一个节点探测失败就返回非零，这是“有节点异常”而非“本机不能继续”的信号，
按节点逐个判断，不要因为一个历史节点的失败阻塞目标节点。

## SSH 与账户

### NAT 容器上 SSH 连不上，或认证一直被拒？

LXC/Incus 类 NAT 实例的 sshd 监听**内网地址**；供应商公网 IP 的 `:22` 常属于网关。

- 实例内 `ss -ltnp` 只看到 `10.x.x.x:22`，就说明没有公网映射。
- 在面板建立显式映射（例如 `公网 <公网端口> → 内网 22`）。
- 公网 `:22` 能完成 TCP 握手不代表认证可用：网关也可能代理握手。

### 怎么证明公网端口落在目标实例而不是网关？

比对 host key 指纹，两边一致才继续：

- 实例内：`ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub`
- 本机：`ssh-keyscan -p <公网端口> -t ed25519 <主机> | ssh-keygen -lf -`

不一致说明映射指向了别的主机，此时任何密码或公钥尝试都无意义。

### 为什么加了 sshd 加固配置却不生效？

OpenSSH 对同一关键字**取第一次出现的值**，
`Include /etc/ssh/sshd_config.d/*.conf` 按文件名顺序读取。
供应商常放 `00-<provider>-auth.conf` 打开 root/密码登录，后写的 `99-hardening.conf` 会被忽略。

- 加固文件必须排序在前，例如 `00-hardening.conf`。
- 用生效值复核，而不是只看配置文本：

```sh
sudo sshd -T | grep -iE '^(permitrootlogin|passwordauthentication)'
sudo sshd -T | grep -iE '^(kbdinteractive|pubkeyauthentication)'
```

- 必须同时验证三条：新用户公钥可登、`root` 公钥被拒、密码/交互认证被拒。

### `rc-service sshd reload` 后立刻连不上正常吗？

reload 后短时间内可能出现 `banner exchange` 超时。等 10–30 秒再判定；
持续失败则回读 `logread` / `/var/log/messages`，不要盲目改端口或反复 reload。

### 新用户公钥正确却被拒，日志写 `account is locked`？

`adduser -D`（Alpine）或未设密码的账户在 `shadow` 中处于锁定状态，
即使公钥正确 sshd 也会拒绝。

- Alpine 没有 `useradd`，用 `adduser -D -s /bin/ash <name>`。
- 设一个随机密码解除锁定（随后仍关闭密码认证）：

```sh
PW=$(head -c 24 /dev/urandom | base64 | tr -d '/+=' | head -c 24)
printf '%s:%s\n' '<name>' "$PW" | chpasswd
```

判定依据是登录成功，不是文件权限看起来正确。

## XP 部署

### `xp-ops deploy` 报 stdin 冲突怎么办？

一次只能提供一个 stdin 密钥：

```text
invalid_args: --join-token-stdin conflicts with --cloudflare-token-stdin
```

Cloudflare token 的解析顺序是
`flag/stdin → CLOUDFLARE_API_TOKEN → /etc/xp-ops/cloudflare_tunnel/api_token`，
把它预先落盘，用 stdin 传 join token。

### 写入前怎么先看到完整计划？

加 `--dry-run`，它会列出将写的路径、创建的用户、改写的 DNS 记录和要跑的服务命令，
但不做任何 POST/PUT/PATCH/DELETE 与文件写入。

### 目标主机名上残留旧 Tunnel/CNAME 会怎样？

直接部署会因“非托管或歧义 DNS 记录”失败，或另建一条重复 Tunnel。
托管路径只在**持久化身份完全匹配**时复用，条件是：

- `/etc/xp-ops/cloudflare_tunnel/settings.json` 的
  `account_id`/`zone_id`/`hostname`/`tunnel_id` 与本次请求一致；
- `/etc/cloudflared/<tunnel_id>.json` 存在且 `TunnelID` 等于该 Tunnel。

可以把这两份身份文件按目标值先落盘再部署；
remote ingress 与 CNAME 仍由 `xp-ops deploy` 在预检后原子改写。

### 托管入口端口要怎么映射？

先看该端口承载什么协议：

- 托管 VLESS/XHTTP 是 TCP，映射 **TCP** 即可；
- 启用了 SS2022 端点时该端口同时承载 UDP，必须建立 **TCP + UDP** 映射；
- SSH 端口只需 TCP。

映射后从外网实测对应协议，不要只凭面板显示“已添加”判定可用。
先看该端口承载什么协议：

- 托管 VLESS/XHTTP 是 TCP，映射 **TCP** 即可；
- 启用了 SS2022 端点时该端口同时承载 UDP，必须建立 **TCP + UDP** 映射；
- SSH 端口只需 TCP。

映射后从外网实测对应协议，不要只凭面板显示“已添加”判定可用。

## 集群成员

### 加入集群报 `503 staged_join_capability_unavailable`？

先分清两种失败：

- voter 缺 `cluster.membership-lifecycle-v1`（加入路径**真正**读的唯一能力门），
  返回 `409 coordinated_upgrade_required`，需要把所有 voter 升级完再重试；
- 能力探测本身失败（Mesh 传输、探测预算、旧公网回落预检），
  返回 `503 staged_join_capability_unavailable`。

`cluster.join.staged-v1` 会出现在能力清单里，但当前加入路径并不强制它，
不要为它单独排查。链路失败不会降级重试：Mesh 探测失败不会改走公网，
voter 的公网地址返回 5xx 同样失败。

- voter 缺 `cluster.membership-lifecycle-v1`（加入路径**真正**读的唯一能力门），
  返回 `409 coordinated_upgrade_required`，需要把所有 voter 升级完再重试；
- 能力探测本身失败（Mesh 传输、探测预算、旧公网回落预检），
  返回 `503 staged_join_capability_unavailable`。

`cluster.join.staged-v1` 会出现在能力清单里，但当前加入路径并不强制它，
不要为它单独排查。链路失败不会降级重试：Mesh 探测失败不会改走公网，
voter 的公网地址返回 5xx 同样失败。

### 怎么只读定位是哪个 voter 阻塞了加入？

逐个 voter 的管理地址直接请求能力清单，看缺哪一项：

```sh
curl -s https://<voter-api>/api/capabilities
```

典型故障是某个 voter 的 Tunnel 挂掉，Cloudflare 返回 `530`。

### 某个 voter 不可达时能怎么处理？

只有两条合法路径，都需要授权：

1. 修复该 voter（供应商侧续费/解封、恢复 Tunnel），再重试加入；
2. 由当前 leader 显式驱逐：`xp-ops xp evict-unreachable-voter --node-id <id>`
   默认 dry-run，再以 `--apply --expected-membership <fingerprint>`
   与对应的 endpoint 参数执行。

不得为了让新节点加入而绕过能力检查、改写 Raft 文件或伪造 voter。

### 驱逐 voter 会连带删掉什么？

dry-run 会列出该节点名下的 endpoint 与 membership 指纹。
`--apply` **无条件**要求 `--delete-endpoints`（即使该节点没有 endpoint）。
有 endpoint 时再带 `--expected-endpoint-ids <id,id>`；
没有 endpoint 时**省略**该参数（不要传空字符串，空串会变成 `[""]` 而触发 409），
`--delete-endpoints` 仍要保留。
缺少 `--delete-endpoints` 是 `invalid_args`；
endpoint 集与 preview 不一致则是 `409 conflict`（endpoint set changed），需要重新跑 dry-run。
operation 完成后节点与其 endpoint 从 DesiredState 消失。

### join token 过期后要重建节点吗？

不用重建节点，但会换一份节点身份。`xp join` 收到 token 过期这类
客户端错误（4xx）时会删除本地 `pending-join.json`；重试时用新 token 生成新的密钥与 CSR。
只有传输错误、超时或 5xx 这类未删除文件的失败，才会复用原身份重试。
若文件仍在而 token 变了，`xp join` 会直接拒绝
（`pending join belongs to another token or node configuration`）。

所以顺序很重要：**先确认全部 voter 可达且能力完整，再签发 token**，否则 token 先过期。

## Runtime 与升级

### 切到 `xp-proxy` 报 `capability_missing` 或服务起不来？

`xp-ops deploy` 会同时安装 `xray` 与 `xp-proxy`；其中 `xp-proxy` 对照 XP release 的
`checksums.txt` 校验，`xray` 来自 `XTLS/Xray-core`，不走该文件。
若匹配版本尚未提供该资产，deploy 只警告并让节点继续用 Xray，不会失败；
`xp-ops install --only xp-proxy` 则把缺失当作硬错误。任何二进制都不会跳过校验安装。
仍然失败时按真实错误串判断：

- `capability_missing: xp-proxy service asset is not installed; run xp-ops init first`
  —— 缺的是 **service 资产**（init 写出的 init 脚本），不是二进制；
  跑 `sudo xp-ops init` 补齐。
- `service_error: selected ingress runtime did not become ready`
  —— 服务起来了又退出。先确认二进制存在且可执行：

```sh
ls -l /usr/local/bin/xp-proxy && xp-ops install --only xp-proxy
```

注意切换失败后 runtime 状态是 `failed`，而 `xp-ops upgrade` 要求 runtime 处于 active，
否则以 `service_error: ingress runtime must be active before upgrade` 中止。
需要升级时就先切回可用 runtime：
`xp-ops deploy` 会同时安装 `xray` 与 `xp-proxy`；其中 `xp-proxy` 对照 XP release 的
`checksums.txt` 校验，`xray` 来自 `XTLS/Xray-core`，不走该文件。
若匹配版本尚未提供该资产，deploy 只警告并让节点继续用 Xray，不会失败；
`xp-ops install --only xp-proxy` 则把缺失当作硬错误；`xp-proxy` 二进制不会跳过校验安装。
仍然失败时按真实错误串判断：

- `capability_missing: xp-proxy service asset is not installed; run xp-ops init first`
  —— 缺的是 **service 资产**（init 写出的 init 脚本），不是二进制；
  跑 `sudo xp-ops init` 补齐。
- `service_error: selected ingress runtime did not become ready`
  —— 服务起来了又退出。先确认二进制存在且可执行：

```sh
ls -l /usr/local/bin/xp-proxy && xp-ops install --only xp-proxy
```

注意切换失败后 runtime 状态是 `failed`，而 `xp-ops upgrade` 要求 runtime 处于 active，
否则以 `service_error: ingress runtime must be active before upgrade` 中止。
需要升级时就先切回可用 runtime：

```sh
sudo xp-ops runtime switch --to xray --yes
sudo xp-ops upgrade --version VERSION
```

`xp-ops upgrade` 没有 `--yes`，不要编造确认参数。

`xp-ops deploy` 不安装 `xp-ops` 自身；`xp` 只在显式 `--xp-bin` 时复制，
`cloudflared` 只在启用 Cloudflare 时处理。

### 切换失败后节点服务全停了怎么办？

`runtime switch` 会先停 `xp` 与旧 runtime。失败时立刻切回可用的那个恢复服务：

```sh
sudo xp-ops runtime switch --to xray --yes
sudo xp-ops runtime status --json
```

### 怎么证明 runtime 切换真的成功？

四条同时对得上，缺一不算成功：

- `xp-ops runtime status --json` 为 `{"runtime":"xp-proxy","state":"active","error":null}`
  （比对 `runtime`/`state`/`error` 三个字段，成功时 `error` 为 `null`）；
- `ss -ltnp` 中托管端口只有 `xp-proxy` 一个 owner，`xray` 未监听；
- `xray` 服务为 stopped，`xp`/`xp-proxy`/`cloudflared` 为 started；
- 本地 canary 返回 204：
  `curl --resolve <access_host>:39043:127.0.0.1 https://<access_host>:39043/generate_204`

## 验收与记录

### 公网入口怎么算通过？

两种协议都要过，且管理入口独立可用：

```sh
curl -sk -o /dev/null -w '%{http_code}\n' --http1.1 https://<access_host>:<port>/generate_204
curl -sk -o /dev/null -w '%{http_code}\n' --http2  https://<access_host>:<port>/generate_204
curl -s -o /dev/null -w '%{http_code}\n' <api-base-url>/api/health
```

期望 `204 / 204 / 200`。

### DDNS 记录还指向旧地址？

XP 接管后会按节点真实出口改写记录。核对本地状态而不是猜：

- `/var/lib/xp/data/ddns_state.json` 的 `snapshot.status` 为 `up`、
  `snapshot.current_ipv4` 为实际出口（字段在 `snapshot` 下，不在顶层）；
- 节点内 `wget -qO- https://cloudflare.com/cdn-cgi/trace | grep ^ip=` 与 DNS 解析一致。

### 审计失败能临时放宽策略吗？

不能。`audit` 的失败要么修节点，要么在私人策略里记录**已获批准**的例外及理由。
为消除失败而放宽策略会让后续节点继承错误基线。

### 这些记录应该放哪里？

- 仓库内：只放脱敏的合同、模板和可复用经验（本目录）。
- 仓库外：别名、地址、域名、端口、凭据、验收记录，放私人运维目录。
