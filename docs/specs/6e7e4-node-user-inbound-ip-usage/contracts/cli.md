# CLI / env

## xp runtime config

Inbound IP usage Geo enrichment no longer exposes dedicated Geo DB env vars. Runtime only requires the existing xp base config plus outbound HTTPS access to `https://api.country.is/`.

## Xray static config requirement

`xp-ops init` 生成的 `/etc/xray/config.json` 必须在 `policy.levels.0` 中同时启用：

- `statsUserUplink=true`
- `statsUserDownlink=true`
- `statsUserOnline=true`

未启用 `statsUserOnline` 时，IP usage 功能进入 warning 模式，不阻断服务主流程。
