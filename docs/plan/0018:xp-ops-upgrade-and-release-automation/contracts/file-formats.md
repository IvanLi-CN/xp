# File formats Contracts（#0018）

本文件用于冻结 `xp`/`xp-ops` 在 GitHub Releases 上的“tag 与 assets 命名/格式”，保证 `xp-ops` 可稳定解析并完成校验。

## Release tag

- Tag 格式（normative）：`v<semver>`（例如 `v0.2.3`）

## Assets naming

每个 release 必须包含以下 assets（normative）：

- `xp-linux-x86_64`
- `xp-linux-aarch64`
- `xp-ops-linux-x86_64`
- `xp-ops-linux-aarch64`
- `checksums.txt`

说明：

- 以上四个二进制 assets 为“裸二进制文件”（不打包 zip/tar.gz），由 `xp-ops` 在安装后设置可执行权限。
- 为最大化兼容性（含 Alpine），上述 Linux assets **必须为静态链接的 musl 构建**（normative）：
  - `*-linux-x86_64` → target `x86_64-unknown-linux-musl`
  - `*-linux-aarch64` → target `aarch64-unknown-linux-musl`
- 若未来需要引入压缩包或签名文件，必须在本文件中以 `Modify` 的方式冻结兼容策略（不在本计划范围内）。

## `checksums.txt` format

- 编码：UTF-8
- 每行格式（normative）：`<sha256-hex>  <filename>`
  - `<filename>` 必须与 assets 名称一致
  - 分隔为两个空格（兼容常见 `sha256sum` 输出）

示例：

```text
0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  xp-linux-x86_64
0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  xp-linux-aarch64
0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  xp-ops-linux-x86_64
0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  xp-ops-linux-aarch64
```

## Compatibility rules

- `xp-ops` 必须拒绝校验文件中缺少目标 asset 的情况（视为 `checksum_mismatch` 或等价错误）。
- `xp-ops` 必须拒绝 sha256 不匹配的下载结果，并不得覆盖目标二进制。
