# Docker 单镜像部署说明

这份文档面向**实际部署者**，描述如何用官方单镜像方式部署 `xp` 集群节点，以及容器入口会自动处理哪些事情。

如果你想看更底层的运行契约、Compose 示例和容器内部行为细节，再看：

- `docs/ops/docker.md`
- `deploy/docker/compose.bootstrap.yml`
- `deploy/docker/compose.join.yml`

## 目标

单镜像部署的目标不是让操作者进入容器里手动执行 `xp init`、`xp join`、`cloudflared tunnel run` 之类命令，而是：

- **通过环境变量声明目标状态**
- **通过卷保存节点持久状态**
- **由 `xp-ops container run` 在启动时自动补齐环境**
- **在重启或环境变量变化后自动把实例重新对齐**

也就是说，正常部署时，操作者只需要：

1. 准备环境变量
2. 挂载固定卷
3. 启动或重启容器

不需要进入容器执行初始化、接入点创建、Tunnel 启动或节点元数据修复。

## 部署模型

一个容器对应一个 `xp` 集群节点。

镜像 `ghcr.io/ivanli-cn/xp` 内已经包含：

- `xp`
- `xp-ops`
- 真实嵌入版 Web 前端
- `xray`
- `cloudflared`
- `tini`

固定入口是：

```bash
xp-ops container run
```

容器入口会负责：

- 判断当前是 **bootstrap 首节点**、**join 新节点**，还是**已有节点重启**
- 在需要时执行 `xp init` 或 `xp join`
- 准备或复用 Cloudflare Tunnel 本地状态
- 启动 `xray`
- 启动 `xp`
- 在 `xp` 启动后，把环境变量声明的节点信息、默认接入点配置重新同步到集群

## 固定卷

必须挂这三个持久化卷：

- `/var/lib/xp/data`
- `/etc/cloudflared`
- `/etc/xp-ops/cloudflare_tunnel`

它们分别保存：

- 集群元数据、证书、Raft 状态、DDNS 状态
- `cloudflared` 凭据与配置
- Tunnel settings（避免重复创建 Tunnel / DNS 资源）

如果不保留这些卷，容器每次都会更接近“全新节点首次部署”。

## 部署前最小检查清单

正式启动前，先确认这几件事：

- 你已经准备好镜像 tag，例如 `ghcr.io/ivanli-cn/xp:latest`
- 三个持久化卷都会保留，不会随着容器删除一起丢掉
- bootstrap 节点准备好了 `XP_ADMIN_TOKEN` 或 `XP_ADMIN_TOKEN_HASH`
- join 节点准备好了 `XP_JOIN_TOKEN`
- 如果开启 Tunnel，已经准备好 `CLOUDFLARE_API_TOKEN`
- 如果启用默认 SS / VLESS，已经决定好对外 hostname、端口和 REALITY 参数

只要这些前提成立，后续部署原则上不需要人工进入容器操作。

## 首次使用 / 重启 / 变更时会发生什么

### 1. 空数据卷，且没有 `XP_JOIN_TOKEN`

容器会把当前节点视为 **bootstrap 节点**：

- 要求提供 `XP_ADMIN_TOKEN` 或 `XP_ADMIN_TOKEN_HASH`
- 自动执行一次初始化
- 后续重启时不会重复 `xp init`

### 2. 空数据卷，且设置了 `XP_JOIN_TOKEN`

容器会把当前节点视为 **join 节点**：

- 先准备或复用 Cloudflare Tunnel
- 等 public `api_base_url` 可达
- 自动执行 `xp join --token ...`
- 后续重启时不会重复 `xp join`

### 3. 数据卷已存在

容器会视为**已有节点重启**：

- 保留原有 `cluster_id` / `node_id`
- 复用已有证书、集群身份和本地 Cloudflare 状态
- 按最新环境变量把操作者可控字段重新对齐

## 环境变量是“目标状态”

单镜像部署的关键思想是：**环境变量描述你想要的节点状态**。

容器入口会在每次启动时尽量把节点对齐到这个目标状态。

### 节点基础身份

至少会涉及这些变量：

- `XP_NODE_NAME`
- `XP_API_BASE_URL`（关闭 Tunnel 时必须显式给）
- `XP_ACCESS_HOST`

如果它们和现有数据卷里的信息不一致：

- 本地 `metadata.json` 会自动改成新的值
- `xp` 启动后，这些值也会同步回集群里的节点元数据

这意味着下面这些变更都属于正常运维动作：

- 节点更名
- 对外 endpoint hostname 变更
- public API hostname 变更

不需要人工进容器修文件。

## Cloudflare Tunnel 与 DDNS

### Tunnel

启用 Tunnel 时，至少提供：

- `XP_ENABLE_CLOUDFLARE=true`
- `XP_CLOUDFLARE_ACCOUNT_ID`
- `XP_CLOUDFLARE_HOSTNAME`
- `CLOUDFLARE_API_TOKEN`

建议同时提供：

- `XP_CLOUDFLARE_ZONE_ID`

可选：

- `XP_CLOUDFLARE_TUNNEL_NAME`

如果不提供 `XP_CLOUDFLARE_TUNNEL_NAME`，默认会使用：

```text
xp-<XP_NODE_NAME>
```

这能避免示例文件里的固定默认值造成 Tunnel 名称冲突。

当 `XP_ENABLE_CLOUDFLARE=true` 且没有显式设置 `XP_API_BASE_URL` 时，默认会派生为：

```text
https://<XP_CLOUDFLARE_HOSTNAME>
```

### DDNS

如果希望容器顺手维护 `XP_ACCESS_HOST` 的 DNS 记录，再加：

- `XP_CLOUDFLARE_DDNS_ENABLED=true`
- `XP_CLOUDFLARE_DDNS_ZONE_ID`

容器入口会在启动 `xp` 之前自动准备 DDNS runtime token file，不需要人工把 token 再复制进容器。

## 默认 SS / VLESS 接入点

部署时可以通过环境变量决定是否初始化默认接入点。

### 默认 VLESS

当你设置这些变量时，容器会托管一个默认 VLESS endpoint：

- `XP_DEFAULT_VLESS_PORT`
- `XP_DEFAULT_VLESS_REALITY_DEST`
- `XP_DEFAULT_VLESS_SERVER_NAMES`
- `XP_DEFAULT_VLESS_FINGERPRINT`（可选，默认 `chrome`）

### 默认 SS2022

当你设置这个变量时，容器会托管一个默认 SS2022 endpoint：

- `XP_DEFAULT_SS_PORT`

### 容器会怎么处理

容器入口在每次启动时都会 reconcile：

- **第一次启动**：如果当前节点没有对应 endpoint，就自动创建
- **后续修改环境变量**：自动 patch 现有托管 endpoint
- **删除相关环境变量**：停止托管并删除自己管理的 endpoint

如果当前节点上某种类型只有**一条**现有 endpoint，容器入口会直接 adopt 这条 endpoint，而不是重复创建。

如果同类型 endpoint 已经有多条，容器入口会拒绝自动接管，避免误伤人工配置。

## bootstrap 节点部署

参考：

- `deploy/docker/compose.bootstrap.yml`

### 最小可用 `.env` 模板

如果你想先用最少变量跑起来，可以从这个模板开始：

```env
XP_IMAGE=ghcr.io/ivanli-cn/xp:latest
XP_CONTAINER_NAME=xp-node-1
XP_NODE_NAME=node-1

XP_ADMIN_TOKEN=replace-with-a-strong-secret

XP_ENABLE_CLOUDFLARE=true
XP_CLOUDFLARE_ACCOUNT_ID=...
XP_CLOUDFLARE_ZONE_ID=...
XP_CLOUDFLARE_HOSTNAME=node-1.example.com

XP_ACCESS_HOST=node-1-ep.example.com
CLOUDFLARE_API_TOKEN=...
```

这份最小模板的含义是：

- 当前节点是首个 bootstrap 节点
- 使用 Cloudflare Tunnel 暴露 API/UI
- 不启用 DDNS
- 不初始化默认 SS / VLESS

如果你希望**同时启用 DDNS 和默认接入点**，再继续加下面这些变量：

```env
XP_CLOUDFLARE_DDNS_ENABLED=true
XP_CLOUDFLARE_DDNS_ZONE_ID=...

XP_DEFAULT_VLESS_PORT=53842
XP_DEFAULT_VLESS_REALITY_DEST=oneclient.sfx.ms:443
XP_DEFAULT_VLESS_SERVER_NAMES=oneclient.sfx.ms,skyapi.onedrive.com
XP_DEFAULT_VLESS_FINGERPRINT=chrome

XP_DEFAULT_SS_PORT=53843
```

如果你**不使用 Tunnel**，那就改成：

```env
XP_ENABLE_CLOUDFLARE=false
XP_API_BASE_URL=https://node-1.example.com
XP_ACCESS_HOST=node-1-ep.example.com
```

这时不需要 `XP_CLOUDFLARE_ACCOUNT_ID`、`XP_CLOUDFLARE_HOSTNAME`、`CLOUDFLARE_API_TOKEN`。

一个典型的 `.env` 可以长这样：

```env
XP_IMAGE=ghcr.io/ivanli-cn/xp:latest
XP_CONTAINER_NAME=xp-node-1
XP_NODE_NAME=node-1

XP_ADMIN_TOKEN=replace-with-a-strong-secret

XP_ENABLE_CLOUDFLARE=true
XP_CLOUDFLARE_ACCOUNT_ID=...
XP_CLOUDFLARE_ZONE_ID=...
XP_CLOUDFLARE_HOSTNAME=node-1.example.com

XP_ACCESS_HOST=node-1-ep.example.com
XP_CLOUDFLARE_DDNS_ENABLED=true
XP_CLOUDFLARE_DDNS_ZONE_ID=...

XP_DEFAULT_VLESS_PORT=53842
XP_DEFAULT_VLESS_REALITY_DEST=oneclient.sfx.ms:443
XP_DEFAULT_VLESS_SERVER_NAMES=oneclient.sfx.ms,skyapi.onedrive.com
XP_DEFAULT_VLESS_FINGERPRINT=chrome
XP_DEFAULT_SS_PORT=53843

CLOUDFLARE_API_TOKEN=...
```

启动后，容器入口会自动完成：

1. 节点初始化
2. Tunnel provisioning / reuse
3. DDNS runtime 准备
4. `xray` 启动
5. `xp` 启动
6. 默认 SS / VLESS endpoint 初始化

## join 节点部署

参考：

- `deploy/docker/compose.join.yml`

### 最小可用 `.env` 模板

如果你要把一个新容器加入现有集群，可以从这个模板开始：

```env
XP_IMAGE=ghcr.io/ivanli-cn/xp:latest
XP_CONTAINER_NAME=xp-node-2
XP_NODE_NAME=node-2

XP_JOIN_TOKEN=replace-with-a-real-join-token

XP_ENABLE_CLOUDFLARE=true
XP_CLOUDFLARE_ACCOUNT_ID=...
XP_CLOUDFLARE_ZONE_ID=...
XP_CLOUDFLARE_HOSTNAME=node-2.example.com

XP_ACCESS_HOST=node-2-ep.example.com
CLOUDFLARE_API_TOKEN=...
```

这份最小模板的含义是：

- 当前节点会作为 join 节点首次加入集群
- 首次启动前先把 Tunnel 准备好
- 不启用 DDNS
- 不初始化默认 SS / VLESS

如果你希望 join 后立即带上默认接入点，再继续加：

```env
XP_DEFAULT_VLESS_PORT=53842
XP_DEFAULT_VLESS_REALITY_DEST=oneclient.sfx.ms:443
XP_DEFAULT_VLESS_SERVER_NAMES=oneclient.sfx.ms,skyapi.onedrive.com

XP_DEFAULT_SS_PORT=53843
```

如果你希望同时维护 `XP_ACCESS_HOST` 的 DDNS，再继续加：

```env
XP_CLOUDFLARE_DDNS_ENABLED=true
XP_CLOUDFLARE_DDNS_ZONE_ID=...
```

一个典型的 `.env`：

```env
XP_IMAGE=ghcr.io/ivanli-cn/xp:latest
XP_CONTAINER_NAME=xp-node-2
XP_NODE_NAME=node-2

XP_JOIN_TOKEN=replace-with-a-real-join-token

XP_ENABLE_CLOUDFLARE=true
XP_CLOUDFLARE_ACCOUNT_ID=...
XP_CLOUDFLARE_ZONE_ID=...
XP_CLOUDFLARE_HOSTNAME=node-2.example.com

XP_ACCESS_HOST=node-2-ep.example.com
XP_CLOUDFLARE_DDNS_ENABLED=true
XP_CLOUDFLARE_DDNS_ZONE_ID=...

XP_DEFAULT_VLESS_PORT=53842
XP_DEFAULT_VLESS_REALITY_DEST=oneclient.sfx.ms:443
XP_DEFAULT_VLESS_SERVER_NAMES=oneclient.sfx.ms,skyapi.onedrive.com
XP_DEFAULT_SS_PORT=53843

CLOUDFLARE_API_TOKEN=...
```

join 场景下，容器入口会自动完成：

1. Tunnel provisioning / reuse
2. public `api_base_url` 健康探测
3. `xp join --token ...`
4. `xray` / `xp` 启动
5. 默认 endpoint 初始化
6. 把环境变量声明的节点信息同步回集群

其中首次 runtime reconcile 会直接复用 `XP_JOIN_TOKEN` 自带的 `leader_api_base_url`，所以不会因为 follower 本地还没学到 leader 路由而卡住。

## 环境变量变更后的预期行为

下面这些都属于支持的正常运维变更：

- 修改 `XP_NODE_NAME`
- 修改 `XP_ACCESS_HOST`
- 修改 `XP_API_BASE_URL`
- 修改 `XP_DEFAULT_VLESS_PORT`
- 修改 `XP_DEFAULT_VLESS_REALITY_DEST`
- 修改 `XP_DEFAULT_VLESS_SERVER_NAMES`
- 修改 `XP_DEFAULT_SS_PORT`

推荐做法是：

1. 改 `.env`
2. 重新 `docker compose up -d`

容器入口会在这次启动里自动完成重新对齐。

## 你不需要做的事情

正常部署和变更时，不应该要求操作者去做这些事：

- `docker exec -it ...`
- 手动运行 `xp init`
- 手动运行 `xp join`
- 手动创建默认 SS / VLESS endpoint
- 手动把 DDNS token 复制到容器内
- 手动修 `metadata.json`
- 手动拉起 `cloudflared`

这些都应该由容器入口根据环境变量和挂载卷自动完成。

## 健康检查

部署完成后，至少应该能看到：

- `GET /api/health` 返回 `200`
- `GET /api/cluster/info` 返回当前节点信息
- `GET /` 能打开嵌入版 Web UI

如果开启了 Tunnel，还应确认：

- `https://<XP_CLOUDFLARE_HOSTNAME>/api/health`
- `https://<XP_CLOUDFLARE_HOSTNAME>/`

如果开启了 DDNS，还应确认：

- `XP_ACCESS_HOST` 对应的 DNS 记录已存在并指向当前节点公网地址

## 常见错误对照表

| 现象                                                        | 常见原因                                                                                                           | 应该怎么处理                                                                         |
| ----------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| 容器一启动就退出，日志提示缺少 bootstrap token              | bootstrap 节点没给 `XP_ADMIN_TOKEN` 或 `XP_ADMIN_TOKEN_HASH`                                                       | 补上其中一个，再重新 `docker compose up -d`                                          |
| 容器一启动就退出，日志提示缺少 join token                   | join 节点没给 `XP_JOIN_TOKEN`                                                                                      | 重新生成或填入正确 token，再重启容器                                                 |
| 开启 Tunnel 后一直起不来                                    | `CLOUDFLARE_API_TOKEN`、`XP_CLOUDFLARE_ACCOUNT_ID`、`XP_CLOUDFLARE_HOSTNAME`、`XP_CLOUDFLARE_ZONE_ID` 其中之一不对 | 先检查这四个值，再看容器日志里的 Cloudflare API 报错                                 |
| 重启后节点像是“变成新机器”了                                | `/var/lib/xp/data` 没持久化，或被换成新卷                                                                          | 保留原数据卷；不要把节点身份卷当临时目录                                             |
| 重启后重复创建 Tunnel / DNS                                 | `/etc/cloudflared` 或 `/etc/xp-ops/cloudflare_tunnel` 没保留                                                       | 把这两个卷改成持久化卷，避免每次首启化                                               |
| `XP_ACCESS_HOST` 没更新到 DNS                               | 没开 `XP_CLOUDFLARE_DDNS_ENABLED=true`，或没给 `XP_CLOUDFLARE_DDNS_ZONE_ID`                                        | 补齐 DDNS 变量，并确认 `CLOUDFLARE_API_TOKEN` 仍可用                                 |
| 默认 VLESS / SS 没自动创建                                  | 没给完整的默认 endpoint 环境变量                                                                                   | VLESS 至少补齐端口、REALITY dest、server names；SS 至少给 `XP_DEFAULT_SS_PORT`       |
| 默认 endpoint reconcile 失败                                | 当前节点上同类型 endpoint 已经有多条，容器入口不敢自动接管                                                         | 先手动清理到只剩一条，或明确决定保留哪条                                             |
| 改了 `.env` 但实例没对齐                                    | 只是改了 env 文件，没有重新启动容器                                                                                | 执行 `docker compose up -d` 让容器入口重新跑一次 reconcile                           |
| `GET /api/health` 正常，但网页运行页看不到 cloudflared 状态 | 容器模式下 `cloudflared` 由容器入口托管，不走 `xp` 内建 runtime supervisor                                         | 这属于预期行为；排查时看容器日志和编排器状态，不看 Web 里的 cloudflared runtime 卡片 |

## 当前推荐的操作者心智模型

把这套部署理解成：

- **Compose / 环境变量**：声明“我想要什么”
- **持久化卷**：保存“这个节点是谁”
- **容器入口**：负责把“当前状态”自动修到“目标状态”

如果这个心智模型成立，部署和变更就会稳定很多，也更符合日常运维预期。
