# CLI / env

## xp runtime config

新增只读配置项：

- `XP_IP_USAGE_CITY_DB_PATH`
  - 默认：空字符串
  - 语义：GeoLite2 City mmdb 文件路径
- `XP_IP_USAGE_ASN_DB_PATH`
  - 默认：空字符串
  - 语义：GeoLite2 ASN mmdb 文件路径

## Xray static config requirement

`xp-ops init` 生成的 `/etc/xray/config.json` 必须在 `policy.levels.0` 中同时启用：

- `statsUserUplink=true`
- `statsUserDownlink=true`
- `statsUserOnline=true`

未启用 `statsUserOnline` 时，IP usage 功能进入 warning 模式，不阻断服务主流程。
