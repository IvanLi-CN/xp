# xp-proxy

`xp-proxy` 是 XP 的独立 Rust 数据面进程。默认模式通过受保护的 Unix socket
接收 snapshot 控制请求；显式传入 `--outbound-config` 时运行 TCP 本地监听器，并通过
VLESS + REALITY + XHTTP `stream-one` + HTTP/2 连接远端 Xray。

## Outbound smoke mode

`--outbound-config` 接受 JSON 文件，字段保持英文协议配置命名：

```json
{
  "listen": "127.0.0.1:12000",
  "remote": "127.0.0.1:18449",
  "server_name": "www.example.com",
  "server_public_key": "<REALITY public key, base64url>",
  "short_id": "0123456789abcdef",
  "private_key": "<ephemeral X25519 private key, base64url>",
  "path": "/xp-xhttp",
  "uuid": "00000000-0000-0000-0000-000000000000",
  "destination_host": "127.0.0.1",
  "destination_port": 80
}
```

该显式模式用于 Rust outbound 与兼容服务端的互操作测试，生产启用仍由 XP 的 capability
和 snapshot 发布路径控制。XHTTP `stream-one` 请求会自动规范化 path 末尾 `/`，并加入
`x_padding` 查询参数以满足 Xray 的默认 padding 校验。
