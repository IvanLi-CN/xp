# 兼容性基线

本矩阵是 native proxy 的发布门禁。只有锁定具体版本、fixture identity 和运行
环境后，才能宣称兼容；协议单测或单向 smoke 不能替代四条路径的
互操作证据。

## 固定版本

- Xray：`26.3.27`，证据来源是
  `scripts/e2e/docker-compose.xray.yml` 的默认 image tag。
- Mihomo：`v1.19.29`，证据来源是
  `scripts/e2e/install-mihomo-v1.19.29.sh`。
- Native protocol：TCP VLESS + REALITY + XHTTP `stream-one`，证据来源是主题
  SPEC 与 golden snapshot。

未明确写入版本、平台、镜像 digest/asset checksum 和测试产物 identity 的运行，
不能成为 release compatibility evidence。

## 互操作矩阵

- **Rust inbound <- Xray outbound：** Xray `26.3.27` VLESS/REALITY/XHTTP client。
  必须证明正确握手、认证、大小 payload、并发 stream、idle reuse，
  以及主动断线后 reconnect。
- **Rust inbound <- Mihomo client：** Mihomo `v1.19.29` YAML 与 URI。
  必须证明 `network: xhttp`、`alpn: [h2]`、固定 path/mode、六项 XMUX、
  连接上限和重连。
- **Rust outbound -> Xray inbound：** Xray `26.3.27` inbound。
  必须证明 Rust client REALITY key schedule、SNI/ALPN/fingerprint、VLESS framing 和
  TCP payload。
- **Rust outbound -> Mihomo-compatible server：** 固定 Mihomo-compatible H2/REALITY
  fixture。必须证明 server compatibility、stream cancellation、错误凭据和远端关闭。

每一方向都必须记录：

- 小包、大包和至少 64 个并发 stream；
- 预热后 idle reuse、主动断开、半关闭和 reconnect；
- UUID、REALITY key、SNI、short ID、ALPN、fingerprint 错误与畸形握手；
- stream cancellation、flow control、上游断开、FD/连接/内存上界；
- 外层 TCP 建连数、H2 transport 数、native session 数和结果分类。

## Fallback 矩阵

fallback 是 endpoint snapshot 的固定配置，不接受客户端任意 target、Host、SNI、URL
或 authority。必须分别验证：

- `http`：协议级 request relay、固定 authority、请求/响应 hop-by-hop header 清理。
- `https`：只向固定 authority 建立 TLS tunnel；错误 SNI/authority 不得扩大目标。
- `h2`：HTTP/2 over TLS 的固定目标、stream 生命周期和 header 过滤。
- `h2c`：cleartext HTTP/2 prior-knowledge 的固定目标、stream 生命周期和 header 过滤。

主人尚未确认四种 scheme 是否都要求协议级语义。
若最终合同只需 raw TCP compatibility，必须显式缩小 scheme 枚举并同步 SPEC；
实现不得静默把四种 scheme 都当作 raw relay。

另需覆盖：目标拒绝、空 authority、authority alias、错误 scheme、redirect。
还需覆盖上游超时、上游断开、malformed request、重复 header 和任意目标注入。

## Deployment evidence

Compatibility Baseline 之外还必须有 Agent VM evidence：

- systemd 独立 `xp-proxy` service 的安装、权限、ready、stop/drain、升级和回滚；
- OpenRC 独立 service 的 `stop -> stopped twice -> start -> ready twice`，以及 crashed
  state、respawn、upgrade 和 rollback；
- Xray/native 同端口冲突、两端口并行、双 owner fail-closed 和 owner handoff；
- proxy crash、XP crash、control socket 消失、陈旧 snapshot、generation conflict 和
  unknown outcome；
- release asset checksum、transaction backup、`rollback_failed`、
  `cleanup_incomplete: committed_upgrade` 和 secret/symlink 安全检查。

## Evidence policy

只有四条互操作路径、fallback、部署、资源和安全 evidence 全部绑定到同一
候选 SHA，并且 `active_ingress` 条件全部成立时，才能把 native proxy
标记为可用。
Xray/Mihomo 的单元测试、现有 XHTTP spike 或“xp-proxy 进程已启动”都不能单独
解锁 production capability。
