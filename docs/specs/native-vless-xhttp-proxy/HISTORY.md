# 主题历史

- 2026-11-14：完成独立数据面设计审查。锁定 XP 控制面与 `xp-proxy` 独立 service、显式
  endpoint owner、单端口唯一 owner、持久 generation、fail-closed 重启、systemd/OpenRC
  生命周期和 Xray/Mihomo 四路径证据门禁。记录 service identity、generation 来源、fallback
  语义、drain SLA、自动 rollback、capability 命名和 Docker 首发范围为主人确认项；未授权
  实现、部署或切换 us3。
- 2026-11-05：确定 native proxy 为第二个独立 Rust 项目。首个生产范围固定为 TCP VLESS、
  REALITY、XHTTP `stream-one`、六项 XMUX 设置和 allowlist HTTP(S) fallback；UDP/XUDP 与
  其他 XHTTP mode 延后到显式 capability 和兼容性证据之后。
