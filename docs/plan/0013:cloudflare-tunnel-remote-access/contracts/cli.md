# CLI Contracts（#0013）

本文件用于冻结“运维 CLI（root 运行）”的最小命令形状与输出口径，用于在目标服务器上安装部署 `xp` / `xray` /（可选）`cloudflared`、完成初始化配置，并提供 TUI 与无交互两种入口。

## Binary

- 可执行文件名：`xp-ops`
- 运行用户：root（或具备等价权限的 sudo）

## Output format

- 默认输出：human-readable（面向终端）
- 可选：`--json` 输出结构化 JSON（便于脚本化与 Web 端集成；实现阶段可选）
- 所有失败必须返回非 0 exit code，并输出可理解的错误信息

## Commands

### `xp-ops install`

说明：安装依赖程序（默认：`xray`；Cloudflare Tunnel 启用时再安装 `cloudflared`），并进行可执行性检查。

Usage:

```bash
xp-ops install \
  [--only cloudflared|xray] \
  [--xray-version <semver|latest>] \
  [--dry-run]
```

Behavior (normative):

- Supported CPU architectures (normative):
  - `x86_64`
  - `aarch64`
  - Other architectures MUST fail with `unsupported_platform`.
- `xray` 默认安装（目标服务器基线组件）。
- `cloudflared` 仅在 Cloudflare Tunnel 启用时安装：
  - 由 `xp-ops cloudflare provision` 触发安装，或显式 `--only cloudflared` 预装。
- `cloudflared` 安装来源（fixed, no heuristics）
  - Arch Linux / Arch Linux ARM: 发行版官方仓库（`pacman`）安装 `cloudflared`
  - Debian: Cloudflare 官方 APT 仓库（`pkg.cloudflare.com`，dist=`any`）安装 `cloudflared`
  - Alpine: 从 Cloudflare 官方 GitHub Releases 下载预编译二进制安装 `cloudflared`（不使用 `apk edge/testing`）
    - `x86_64` → asset `cloudflared-linux-amd64`
    - `aarch64` → asset `cloudflared-linux-arm64`
  - 安装路径（normative）：
    - Arch Linux / Arch Linux ARM / Debian：由包管理器提供（通常为 `/usr/bin/cloudflared`），`xp-ops` 生成服务文件时必须使用绝对路径
    - Alpine：`/usr/local/bin/cloudflared`
  - Debian APT 仓库配置（normative）：
    - GPG key MUST 下载到：`/usr/share/keyrings/cloudflare-main.gpg`（URL: `https://pkg.cloudflare.com/cloudflare-main.gpg`）
    - APT source MUST 写入：`/etc/apt/sources.list.d/cloudflared.list`，内容为：
      - `deb [signed-by=/usr/share/keyrings/cloudflare-main.gpg] https://pkg.cloudflare.com/cloudflared any main`
- `xray` 安装策略
  - 默认且唯一支持：从 Project X 官方 GitHub Releases 下载 zip 并安装。
    - 说明：Project X 上游文档在 Arch Linux 仅给出 AUR/archlinuxcn（非发行版官方源），Debian/Alpine 未提供发行版包管理器方案，因此 v1 不依赖发行版包管理器安装。
  - `--xray-version`：
    - `latest`：安装上游 latest release
    - `<semver>`：安装指定版本（便于复现与回滚）
  - 安装路径（normative）：`/usr/local/bin/xray`
  - Linux release asset mapping（normative）：
    - `x86_64` → asset `Xray-linux-64.zip`
    - `aarch64` → asset `Xray-linux-arm64-v8a.zip`
  - 上游版本/下载信息来源（实现阶段需固化并可被 mock）：GitHub Releases（`XTLS/Xray-core`）元数据；实现阶段必须支持 HTTP mock 以便自动化测试。
- `--dry-run`：不实际执行安装命令，仅打印将要执行的命令与检测点（用于自动化测试与预演）。
- 安装完成后执行版本探测，确认 binary 可用：
  - `xray`：`xray version` 或 `xray -version`
  - `cloudflared`：`cloudflared --version`（或等价）

Exit codes:

- `0`: success
- `2`: unsupported_platform
- `3`: install_failed
- `4`: verification_failed

### `xp-ops init`

说明：在目标服务器初始化运行用户/目录/权限，并生成（可选启用）自启动服务文件；同时生成 `xray` 的最小安全配置模板（不开放任何入站）。

Usage:

```bash
xp-ops init \
  [--xp-work-dir /var/lib/xp] \
  [--xp-data-dir /var/lib/xp/data] \
  [--xray-work-dir /var/lib/xray] \
  [--init-system auto|systemd|openrc|none] \
  [--enable-services] \
  [--dry-run]
```

Behavior (normative):

- 确保以下运行用户/组存在，不存在则创建（实现阶段定案：仅创建 system user，不创建 home）：
  - `xp:xp`（运行 `xp`）
  - `xray:xray`（运行 `xray`）
  - `cloudflared:cloudflared`（仅当 Cloudflare Tunnel 启用时需要；运行 `cloudflared`）
- 创建目录：
  - `--xp-work-dir` 与 `--xp-data-dir`
  - `--xray-work-dir`
  - `/etc/xray/`（用于生成 `/etc/xray/config.json`）
  - `/etc/xp/`（用于可选的 `/etc/xp/xp.env`）
  - `/etc/xp-ops/cloudflare_tunnel/`（用于 settings/api_token；见 `contracts/config.md`）
  - `/etc/cloudflared/`（用于 `<tunnel-id>.json` 与 `config.yml`；见 `contracts/config.md`）
- 设置最小权限：
  - secret files MUST 为 `0600`
  - `--xp-work-dir`/`--xp-data-dir` owner/group 为 `xp:xp`
  - `--xray-work-dir` owner/group 为 `xray:xray`
- init system：
  - `auto`：按发行版默认判断（Arch/Debian → systemd；Alpine → OpenRC）
  - `none`：仅初始化目录与权限，不生成服务文件
- 自启动（若 `--enable-services`）：
  - systemd：写入 unit 文件并 `systemctl enable --now ...`
  - OpenRC：确保 `/etc/init.d/<name>` 存在（可由发行版包提供；若缺失则由 `xp-ops` 生成），并按需写入/更新 `/etc/conf.d/<name>`，然后 `rc-update add ...`（是否立即启动由实现阶段定案）
  - 默认生成并启用：`xp`、`xray`
  - 仅当启用 Cloudflare Tunnel 时生成并启用：`cloudflared`
- `--dry-run`：不落盘、不执行系统命令，仅打印将要创建的目录/文件与将要执行的命令（用于自动化测试与预演）。

Exit codes:

- `0`: success
- `2`: invalid_args
- `3`: permission_denied
- `4`: filesystem_error

### `xp-ops xp install`

说明：在目标服务器上安装/更新 `xp` 可执行文件，并（可选）启用自启动服务。

Usage:

```bash
xp-ops xp install --xp-bin <path> [--enable] [--dry-run]
```

Behavior (normative):

- 将 `--xp-bin` 复制到 `/usr/local/bin/xp`（原子替换），设置可执行权限。
- 当 `--enable`：启用并启动 `xp` 服务（systemd/OpenRC；按 `xp-ops init --init-system` 决定；若服务文件不存在则失败并提示先运行 `xp-ops init`）。
- `--dry-run`：不落盘、不启用服务，仅打印将要执行的文件变更与系统命令（用于自动化测试与预演）。

Exit codes:

- `0`: success
- `2`: invalid_args
- `3`: filesystem_error
- `4`: service_error

### `xp-ops xp bootstrap` (new)

说明：为 `xp` 做一次性引导初始化（首次部署用）：写入节点信息与对外地址，并调用 `xp init` 在 `XP_DATA_DIR` 下生成 cluster metadata + CA/证书。

Usage:

```bash
xp-ops xp bootstrap \
  --node-name <name> \
  --public-domain <domain> \
  --api-base-url <https-origin> \
  [--dry-run]
```

Behavior (normative):

- 必须确保 `XP_DATA_DIR` 已存在且 owner/group 为 `xp:xp`（由 `xp-ops init` 负责创建）。
- 必须以 `xp` 用户身份执行 `xp init`（避免生成的 `XP_DATA_DIR/cluster/*` 文件归属为 root，导致 `xp` 进程无法读取）。
- `--api-base-url` MUST 以 `https://` 开头，且为完整 origin（不带 path）。
- 幂等：
  - 若 `XP_DATA_DIR/cluster/metadata.json` 已存在，应输出“已初始化”并退出成功（或执行一致性校验后退出成功）；不得重复初始化覆盖。
- `--dry-run`：不实际调用 `xp init`，仅打印将要执行的用户切换与命令（用于自动化测试与预演）。

Exit codes:

- `0`: success
- `2`: invalid_args
- `3`: xp_not_installed
- `4`: filesystem_error
- `5`: xp_init_failed

### `xp-ops deploy` (new, recommended)

说明：一键部署（面向“尽快部署成功”）：串联 `install` → `init` → `xp install` → `xp bootstrap` →（可选）`cloudflare provision`，并按策略启用服务。

Usage:

```bash
xp-ops deploy \
  --xp-bin <path> \
  --node-name <name> \
  --public-domain <domain> \
  [--cloudflare | --no-cloudflare] \
  [--account-id <id> --zone-id <id> --hostname <fqdn> --origin-url <url> --path '*'] \
  [--api-base-url <https-origin>] \
  [--xray-version <semver|latest>] \
  [--enable-services | --no-enable-services] \
  [--dry-run]
```

Behavior (normative):

- 默认行为以“快速部署成功”为准：
  - 必须安装依赖并初始化目录/权限。
  - 必须安装 `xp` 并启用 `xp`、`xray` 自启动服务。
  - Cloudflare Tunnel 默认启用：
    - 若缺少 `account_id/zone_id/hostname/origin_url` 或 token，必须失败并给出下一步提示（例如改用 `--no-cloudflare` 或先设置 token/参数）；不得静默跳过。
- `xp` 引导（bootstrap）：
  - 若 `--cloudflare`：`api_base_url` MUST 推导为 `https://<hostname>`（默认 443；不得携带自定义端口），并调用 `xp-ops xp bootstrap`。
  - 若 `--no-cloudflare`：必须显式提供 `--api-base-url <https-origin>`，并调用 `xp-ops xp bootstrap`。
- `--no-cloudflare`：跳过 Cloudflare API provisioning 与 `cloudflared` 服务启用。
- 依赖安装来源：与 `xp-ops install` 保持一致（同一套 actions）：
  - `xray`：固定为官方 GitHub Releases 下载；`--xray-version` 用于 pin 版本
  - `cloudflared`：按发行版固定来源（Arch pacman / Debian Cloudflare APT / Alpine GitHub Releases）
- 幂等：重复执行应复用既有 `settings.json`（tunnel_id/dns_record_id）并确保最终状态一致。
- `--dry-run`：不调用 Cloudflare API、不落盘、不启用服务；仅打印将要执行的步骤清单与关键命令（用于自动化测试与预演）。

Exit codes:

- `0`: success
- `2`: invalid_args
- `3`: install_failed
- `4`: filesystem_error
- `5`: cloudflare_error
- `6`: service_error

### `xp-ops status` (optional)

说明：展示依赖程序与关键路径是否就绪。

Usage:

```bash
xp-ops status [--json]
```

Minimum output fields (when `--json`):

```jsonc
{
  "xp": { "present": true, "version": "x.y.z", "path": "/usr/local/bin/xp" },
  "cloudflared": { "present": true, "version": "x.y.z", "path": "/usr/bin/cloudflared" },
  "xray": { "present": true, "version": "x.y.z", "path": "/usr/local/bin/xray" },
  "xp_work_dir": { "path": "/var/lib/xp", "exists": true, "owner": "xp", "group": "xp" },
  "xp_data_dir": { "path": "/var/lib/xp/data", "exists": true, "owner": "xp", "group": "xp" }
}
```

## Security notes (normative)

- CLI 需要读取 Cloudflare API token（用于 provision/status 等调用外部 API）；允许以 root-only 形式落盘（见 `contracts/config.md`），并提供清除/轮换能力；tunnel credentials file 写入本机 secret 文件供 `cloudflared` 使用。
- CLI 的日志不得包含敏感路径内容的 file dumps（只输出路径与权限摘要）。

## Cloudflare provisioning

### `xp-ops cloudflare token set` (new)

说明：保存/覆盖 Cloudflare API token（root-only），供 `xp-ops cloudflare provision/status` 使用。

Usage:

```bash
xp-ops cloudflare token set [--from-stdin | --from-env CLOUDFLARE_API_TOKEN] [--dry-run]
```

### `xp-ops cloudflare provision`

说明：通过 Cloudflare API 创建/更新 tunnel、写入 ingress、创建/校验 DNS，并把 `credentials_file` 写入 `/etc/cloudflared/<tunnel-id>.json`；随后生成 `/etc/cloudflared/config.yml` 并可（重）启用 `cloudflared` 自启动服务。

Usage:

```bash
xp-ops cloudflare provision \
  --account-id <id> \
  --zone-id <id> \
  --hostname <fqdn> \
  --origin-url <url> \
  [--path '*'] \
  [--enable | --no-enable] \
  [--dry-run]
```

Input (token):

- 默认从 `/etc/xp-ops/cloudflare_tunnel/api_token` 读取（见 `contracts/config.md`）
- 或通过 `CLOUDFLARE_API_TOKEN` 环境变量提供（推荐用于 CI/一次性执行）
  - 禁止在命令行参数中直接传 token（避免出现在 shell history/ps）

Behavior (normative):

- 幂等：重复执行时应复用 `settings.json` 中的 `tunnel_id`/`dns_record_id`（若存在），否则创建新的。
- 维护 `settings.json`：MUST 写入/更新（见 `contracts/config.md`）：
  - `enabled`（与 `--enable/--no-enable` 一致）
  - `account_id/zone_id/hostname/origin_url/path`
  - `tunnel_id/dns_record_id`（用于幂等重跑）
- 始终确保最终 ingress 为：
  - `hostname + path="*"` → `origin_url`
  - catch-all → `http_status:404`
- DNS：确保 `hostname` 存在 CNAME 到 `${tunnel_id}.cfargotunnel.com`，且 `proxied=true`
- 本地文件：MUST 写入/更新（见 `contracts/config.md`）：
  - `/etc/cloudflared/<tunnel-id>.json`（credentials file, `0600`）
  - `/etc/cloudflared/config.yml`（引用 `tunnel` 与 `credentials-file`）
- 默认行为：等价于 `--enable`（即 provisioning 成功后立即启用并启动 `cloudflared` 自启动服务）。
- provisioning 前必须确保 `cloudflared` 已安装可执行（若缺失，按 `xp-ops install` 的策略安装；仍不满足则失败并给出提示）。
- 当 `--enable`：
  - systemd：`systemctl enable --now cloudflared`
  - OpenRC：`rc-update add cloudflared` 并启动（实现阶段定案是否立即启动）
- 当 `--no-enable`：
  - 只完成 provisioning 与本地配置落盘，不启用/不启动 `cloudflared` 服务。
- `--dry-run`：不调用 Cloudflare API、不落盘、不启用服务；仅打印将要发起的 HTTP 请求与将要落盘的变更（用于自动化测试与预演）。

Exit codes:

- `0`: success
- `2`: invalid_args
- `3`: token_missing
- `4`: cloudflare_api_error
- `5`: dns_error
- `6`: filesystem_error

## TUI (Terminal UI)

### `xp-ops tui`

说明：交互式运维界面（Terminal UI），用于在终端中以向导形式完成 install/init/provision/service enable 等流程。TUI 仅是交互层；底层执行与非交互命令一致。

Usage:

```bash
xp-ops tui
```

Screens (v1):

- Overview：显示当前环境探测（OS、init system）、依赖程序状态、工作目录/权限状态
- Deploy（recommended）：一键部署向导（收集必要参数并串联 install/init/xp install/cloudflare provision）
- Install：选择并执行依赖安装（cloudflared/xray）
- Init：初始化目录与权限（user/group、init system auto）
- XP：安装/更新 `xp` 可执行文件（从本地路径选择/粘贴路径；实现阶段定案交互形态）
- Cloudflare：输入 `account_id/zone_id/hostname/origin_url`，并执行 provision
  - Token input：读取 `/etc/xp-ops/cloudflare_tunnel/api_token` 或 `CLOUDFLARE_API_TOKEN`，或在 TUI 内手工粘贴（mask）；**默认保存**到 `/etc/xp-ops/cloudflare_tunnel/api_token`（root-only），并允许在界面上显式关闭“保存 token”。
- Services：启用/禁用并启动/停止 systemd/OpenRC 服务（xp/cloudflared/xray）
- Logs (optional)：展示关键日志摘要（脱敏）

Keybindings (v1, normative):

- `q` / `Esc`: quit（若存在未应用变更，需要二次确认）
- `Tab` / `Shift+Tab`: focus switch
- `Enter`: confirm / run action
- `↑↓`/`jk`: navigate list

Errors:

- 所有失败必须在界面上展示错误码与可读信息，并提供“复制诊断信息”（redacted）能力（实现阶段可选）。
