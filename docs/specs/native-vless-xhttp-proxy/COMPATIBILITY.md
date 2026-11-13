# 兼容性基线

本矩阵是 native proxy 的发布门禁。只有锁定具体版本并接受对应 fixture 后，才能宣称兼容。

| 面向          | 基线                   | 必须提供的证据                                   |
| ------------- | ---------------------- | ------------------------------------------------ |
| Xray inbound  | 锁定的 Xray release    | Rust outbound 能连接并传输 TCP payload           |
| Xray outbound | 锁定的 Xray release    | Rust inbound 能认证并转发 TCP payload            |
| Mihomo client | 锁定的 Mihomo release  | XHTTP YAML 和 URI 能连接 Rust inbound            |
| REALITY       | 基线 Xray REALITY 配置 | 正确和错误握手向量                               |
| XHTTP         | `stream-one`、HTTP/2   | request/response streaming 和 cancellation       |
| XMUX          | XP 六项设置            | reuse、concurrency、expiry、keepalive、reconnect |
| fallback      | XP 固定合同            | canary、HTTP/HTTPS、H2、h2c、authority rejection |

在记录确切版本值和测试产物身份之前，不得作出兼容性声明。
