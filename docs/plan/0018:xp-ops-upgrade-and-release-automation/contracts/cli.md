# CLI Contracts（#0018）

本文件用于冻结“升级相关命令”的最小形状与输出口径，确保 `xp-ops` 与 Release 产物之间有稳定契约。

## Binary

- 可执行文件名：`xp-ops`
- 运行用户：root（或具备等价 sudo 权限）

## Output format

- 默认输出：human-readable（面向终端）
- 所有失败必须返回非 0 exit code，并输出可理解、可诊断的错误信息
- `--dry-run`：不下载、不写盘、不停服务，仅输出将要执行的动作清单与解析到的 release 信息

## Supported platforms

仅对下列平台提供升级支持（normative）：

- `linux-x86_64`
- `linux-aarch64`

其它平台必须失败并返回 `unsupported_platform`。

## Commands

### `xp-ops self-upgrade` (new)

说明：从 GitHub Releases 下载匹配平台的 `xp-ops` 二进制并升级自身。

Usage:

```bash
xp-ops self-upgrade \
  [--version <semver|latest>] \
  [--prerelease] \
  [--repo <owner/repo>] \
  [--dry-run]
```

Behavior (normative):

- `--version`：
  - `latest`：默认使用 GitHub `releases/latest`（stable；不包含 prerelease）
  - `<semver>`：使用 tag `v<semver>`
- `--prerelease`：
  - 仅当 `--version=latest` 时生效：改为选择“最新 prerelease”（通过 `releases` 列表过滤 `prerelease=true` 并取最新 published）。
  - 当 `--version` 为具体版本（包含 prerelease 的 semver，如 `1.2.0-rc.1`）时，不需要该参数。
- 下载的产物必须做 sha256 校验（校验文件与格式见 `contracts/file-formats.md`）。
- 目标安装路径（normative）：
  - 默认：替换当前正在运行的 `xp-ops` 可执行文件路径（`current_exe`）
  - 若目标路径不可写：必须失败并提示“权限不足/安装路径不可写”
- 必须保留旧版本副本用于回滚（normative）：
  - 覆盖前将旧文件移动为 `<path>.bak`（或 `<path>.bak.<timestamp>`，实现阶段二选一并固化）
  - 若替换成功但后续校验失败（例如无法执行 `xp-ops --version`）：必须回滚

Exit codes (normative):

- `0`: success（包含“已是最新版本”的情况）
- `2`: unsupported_platform
- `3`: invalid_args
- `4`: permission_denied
- `5`: download_failed
- `6`: checksum_mismatch
- `7`: install_failed

### `xp-ops xp upgrade` (new)

说明：从 GitHub Releases 下载匹配平台的 `xp` 二进制，安装到 `/usr/local/bin/xp` 并重启服务使其生效；失败回滚旧二进制。

Usage:

```bash
xp-ops xp upgrade \
  [--version <semver|latest>] \
  [--prerelease] \
  [--repo <owner/repo>] \
  [--dry-run]
```

Behavior (normative):

- `--version` 与 release 解析规则同 `self-upgrade`。
- 必须做 sha256 校验（见 `contracts/file-formats.md`）。
- 安装路径（normative）：`/usr/local/bin/xp`（原子替换，确保可执行权限）。
- 服务管理（normative）：
  - 必须尝试 stop/restart `xp` 服务使新版本生效（systemd/OpenRC 二选一，按运行环境检测）。
  - 若重启失败，必须回滚到旧二进制，并尝试恢复旧服务状态（至少可再次启动）。
- 必须保留旧版本副本用于回滚（同 `self-upgrade` 的备份策略）。

Exit codes (normative):

- `0`: success
- `2`: unsupported_platform
- `3`: invalid_args
- `4`: permission_denied
- `5`: download_failed
- `6`: checksum_mismatch
- `7`: service_error
- `8`: rollback_failed
