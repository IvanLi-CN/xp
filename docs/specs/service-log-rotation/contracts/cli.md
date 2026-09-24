# 命令行（CLI）

## `xp-ops init`（Modify）

- 范围（Scope）: external
- 变更（Change）: Modify

### 用法（Usage）

```text
xp-ops init [existing options...] \
  [--disable-log-rotation] \
  [--log-rotation-max-size <SIZE>] \
  [--log-rotation-max-age-days <DAYS>] \
  [--log-rotation-schedule <SCHEDULE>] \
  [--log-rotation-target <TARGET> ...]
```

### 参数（Args / options）

- `--disable-log-rotation`: 关闭并卸载“本项目日志文件轮换”（默认：false；即默认开启）。
- `--log-rotation-max-size <SIZE>`: 单文件上限（默认：`100MiB`）。
- `--log-rotation-max-age-days <DAYS>`: 保留天数（默认：`7`）。
- `--log-rotation-schedule <SCHEDULE>`: 触发频率（默认：`daily`）。该值在 systemd（Arch/Debian）与 Alpine periodic 间做等价映射。
- `--log-rotation-target <TARGET>`: 指定需要轮换的日志组（可重复）。候选值由实现冻结：
  - `xray`（至少包含 xray access/error logs）
  - `xp`（仅当启用“stdout/stderr 落盘到文件”或已知存在文件日志）
  - `cloudflared`（同上）

约束：

- 默认只生成/管理“本项目日志”对应的 logrotate 配置；不得触碰系统其他日志配置。
- 默认不压缩轮换文件（不启用 `compress`）。
- 默认安装并启用“本项目专用 runner”（systemd timer / Alpine periodic），runner 必须仅作用于 `/etc/logrotate.d/xp-ops`，且使用独立的 status 文件。
- 当设置 `--disable-log-rotation`：
  - 必须卸载本计划写入的全部 artifacts（配置文件、runner、status 文件），并确保 runner 已 stop/disable。
  - 该卸载必须幂等：重复执行无副作用。
  - 该卸载不得要求 `logrotate` 存在（允许在缺依赖机器上清理 artifacts）。
  - 必须保证“不影响其他项目”：不得改写系统全局 logrotate 策略或 journald 策略。
- 当 `--log-rotation-target xray`：
  - `xp-ops` 必须尝试读取 `/etc/xray/config.json` 并解析 `log.access` / `log.error` 字段作为轮换目标（若存在且为有效路径、且不为 `none`）。
  - 若上述字段缺失/为空：使用默认路径 `/var/log/xray/access.log` 与 `/var/log/xray/error.log`。
- `--dry-run` 必须打印将写入的配置文件路径与要点，但不得写入/执行任何系统动作。

### 输出（Output）

- Format: human
- 成功时打印：写入/更新的文件列表；（若启用 runner）如何检查 runner 状态；以及如何手工执行一次轮换用于验证。
- 失败时打印：错误码 + 单句原因 + 建议修复动作（例如安装 `logrotate`、修正权限、提供目标日志路径）。

### 退出码（Exit codes）

- `0`: 成功
- `2`: preflight/detect error（例如不支持的平台/缺少必要组件）
- `4`: filesystem error（写入失败）
- `5`: invalid usage（参数非法或无法满足约束）

### 兼容性与迁移（Compatibility / migration）

- 兼容性：不传 `--disable-log-rotation` 时默认启用日志轮换配置与 runner（新行为）；如需保持旧行为，显式传 `--disable-log-rotation`。
- 参数新增必须保持向后兼容；默认值变化必须同步文档并可显式覆盖。
