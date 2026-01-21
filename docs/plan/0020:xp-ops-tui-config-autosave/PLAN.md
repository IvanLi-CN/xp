# `xp-ops` TUI：部署前自动保存配置 + 退出保存确认（#0020）

## 状态

- Status: 已完成
- Created: 2026-01-20
- Last: 2026-01-21

## 背景 / 问题陈述

- `xp-ops tui` 当前提供 Deploy wizard：可编辑一组部署参数，并支持显式保存（`s` / `S`）与一键 deploy（`d`）。
- 现状下，用户在 TUI 中修改参数后如果直接 deploy 或退出，存在“配置未持久化（或被误丢弃）”的风险，导致：
  - deploy 可能与用户以为的配置不一致（尤其是下一次打开 TUI 时仍是旧值）。
  - 退出时无确认，容易误操作丢失编辑内容。

## 目标 / 非目标

### Goals

- 避免“未保存的 TUI 配置”导致 deploy 与预期不一致：在 deploy 前自动保存。
- 避免误退出丢失改动：退出时在存在未保存改动的情况下进行保存确认。
- 改善可操作性：使用 Tab / 上下箭头 / 鼠标点击切换焦点与编辑项；使用 Ctrl 组合键执行命令操作。
- 保持对敏感信息（Cloudflare token）的默认保护：不在 UI/日志中泄露 token 值。

### Non-goals

- 不改变 `xp-ops deploy` 的核心部署语义与行为（仅补齐 TUI 层的保存与退出交互）。
- 不改变 `/etc/xp-ops/deploy/settings.json` 的核心字段与含义（仍兼容 alias）；允许弃用并不再写出历史字段 `save_token`（读取时继续忽略/兼容）。
- 不进行 `xp-ops` 的语言重写（保持 Rust 实现与现有测试/发布方式）。
- 不新增新的配置来源（例如远端配置中心 / DB）；不引入/升级依赖。

## 用户与场景

- 运维人员使用 `xp-ops tui` 填写或调整 deploy 参数：
  - 场景 A：编辑完参数后直接 deploy，希望 deploy 一定基于当前屏幕上的值。
  - 场景 B：编辑完参数后退出，希望能被提醒是否保存，以便下次复用参数或避免误丢弃。

## 需求（Requirements）

### MUST

- MUST: `xp-ops tui` 的“命令操作”必须使用 Ctrl 组合键触发（避免误触）：
  - `Ctrl+S`: 保存
  - `Ctrl+D`: 自动保存后开始 deploy
  - `Ctrl+Q`: 退出（若 dirty 则进入保存确认）
- MUST: 在 `xp-ops tui` 中触发 deploy（`Ctrl+D`）前，必须自动保存 TUI 配置到 `/etc/xp-ops/deploy/settings.json`（见 `src/ops/paths.rs`），以保证 deploy 逻辑与 UI 当前值一致。
- MUST: 自动保存失败时，必须阻止 deploy（不进入 deploy 流程），并在 TUI 内给出可定位的错误提示。
- MUST: 即使 `dry_run=true`，触发 deploy（`Ctrl+D`）前仍必须自动保存上述配置（写盘行为保持一致，减少“dry-run 和 real-run 口径不一致”带来的困惑）。
- MUST: 在 `xp-ops tui` 中退出（`Ctrl+Q`）时，如果存在未保存的更改，必须弹出保存确认（至少包含：保存并退出 / 不保存直接退出 / 取消退出）。
- MUST: “是否存在未保存更改”的判断必须可预测且可测试：
  - 与“上一次成功保存的状态”（或启动时加载的默认/已有配置）一致则不提示；
  - 仅当与基准不同才提示。
- MUST: 保存操作的输出与敏感信息处理保持现有约束：不打印 token 值；UI 仅显示 `(saved)` 或掩码。
- MUST: token 与其他字段一致：
  - TUI 不提供“是否保存 token”的独立开关；
  - 保存（显式保存或 deploy 前 autosave）时，若 token 输入框非空，则持久化到 `/etc/xp-ops/cloudflare_tunnel/api_token`；
  - 保存（显式保存或 deploy 前 autosave）时，若 token 输入框为空，则**保持现有 token 不变**（不清空/不删除文件）；
  - token 的变化必须计入 dirty state（退出需确认）。
- MUST: 焦点/编辑切换必须支持：
  - Tab / Shift+Tab
  - 上下箭头
  - 鼠标点击（左键）选择字段/切换编辑项
- MUST: 更新 `xp-ops tui` 的帮助提示（Help 文案）以反映“退出会在有改动时询问保存”“deploy 会自动保存”的行为。

### SHOULD

None

### COULD

None

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name）                              | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc）                                 | 负责人（Owner） | 使用方（Consumers） | 备注（Notes）                                      |
| ----------------------------------------- | ------------ | ------------- | -------------- | -------------------------------------------------------- | --------------- | ------------------- | -------------------------------------------------- |
| `xp-ops tui`                              | CLI          | external      | Modify         | [contracts/cli.md](./contracts/cli.md)                   | ops             | operators           | key bindings + quit/deploy 行为补齐                |
| `/etc/xp-ops/deploy/settings.json`        | File format  | external      | Modify         | [contracts/file-formats.md](./contracts/file-formats.md) | ops             | operators           | 字段形状不变；写入时机发生变化                     |
| `/etc/xp-ops/cloudflare_tunnel/api_token` | File format  | external      | Modify         | [contracts/file-formats.md](./contracts/file-formats.md) | ops             | operators           | token 保存策略调整为“与其他字段一致（无单独开关）” |

### 契约文档（按 Kind 拆分）

- [contracts/README.md](./contracts/README.md)
- [contracts/cli.md](./contracts/cli.md)
- [contracts/file-formats.md](./contracts/file-formats.md)

## 约束与风险

### 约束（Constraints）

- `xp-ops` 的配置文件默认位于 `/etc/xp-ops/**`，写入通常需要 root 权限；自动保存必须对“权限不足/目录不存在/只读文件系统”等错误给出清晰提示。
- `xp-ops tui` 运行在 raw mode/alternate screen；退出确认需要在 TUI 内完成，避免落回 shell 交互导致状态错乱。

### 风险（Risks）

- 自动保存会引入“隐式写入”的行为：需要明确其边界（例如在 `dry_run` 模式下是否仍会写入本地 TUI 配置）。
- 若退出确认/状态机实现不当，可能造成用户无法退出或误保存；需要可测试的状态机设计。

## 验收标准（Acceptance Criteria）

- Given 在 `xp-ops tui` 中修改任意字段且尚未保存
  When 按下 `Ctrl+D` 触发 deploy
  Then 必须先完成自动保存（`/etc/xp-ops/deploy/settings.json` 落盘成功），随后才开始 deploy。
- Given 当前环境无法写入 `/etc/xp-ops/deploy/settings.json`（例如权限不足）
  When 按下 `Ctrl+D`
  Then 不应开始 deploy，且 TUI 内出现明确错误提示（可用于定位原因）。
- Given 在 `xp-ops tui` 中存在未保存更改
  When 按下 `Ctrl+Q`
  Then 出现保存确认：
  - 按 `Esc` / `Enter` 选择“取消退出”，应回到 TUI；
  - 按 `Ctrl+Q` 选择“退出不保存”，应直接退出且不写入 settings；
  - 按 `Ctrl+S` 选择“保存并退出”，应保存成功后退出；保存失败应留在 TUI 并展示错误。
- Given 当前无未保存更改
  When 按下 `Ctrl+Q`
  Then 应直接退出，不出现确认提示。
- Given `settings.json` 使用旧字段名 `public_domain`
  When 打开 `xp-ops tui`
  Then `access_host` 应正确加载该值（兼容性不回退）。
- Given `/etc/xp-ops/cloudflare_tunnel/api_token` 已存在且 token 输入框为空
  When 执行保存或 deploy 前 autosave
  Then token 文件必须保持不变（不清空、不删除、不覆盖为空）。

## 实现前置条件（Definition of Ready / Preconditions）

- 已由主人确认：“配置”的范围仅指 `xp-ops tui` 的 deploy settings（`/etc/xp-ops/deploy/settings.json`），不包含 `xp` 服务运行配置（如 `/etc/xp/xp.env`）。
- 已由主人确认：“自动保存”在 `dry_run=true` 时**仍写入** deploy settings（写盘行为保持一致）。
- 已由主人确认：Cloudflare token 的改动计入“未保存更改”判断（避免误退出丢失输入）。
- 退出确认的交互口径（按钮/快捷键/默认选项）已由主人确认，并已固化到 CLI 契约。
- 已由主人确认：命令操作使用 Ctrl 组合键触发（`Ctrl+S`/`Ctrl+D`/`Ctrl+Q`），并支持 Tab/箭头/鼠标切换焦点与编辑项。
- 已由主人确认：token 输入框为空时保存/自动保存**保持现有 token 不变**。
- 契约文档已定稿（本计划以 `./contracts/*.md` 为准），实现与测试可直接按契约落地。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Unit tests: 覆盖 TUI “dirty state” 判定与 key 处理状态机（无需真实终端，避免 flake）。
- Integration tests: 覆盖保存落盘（`--root` 重定向写路径）与错误码/错误信息稳定性（至少 1 个失败场景）。

### Quality checks

- Rust: `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test`
- Docs/Markdown: `bunx --no-install dprint fmt`（按仓库既有约定）

## 文档更新（Docs to Update）

- `docs/ops/README.md`: 增补 `xp-ops tui` 的推荐用法与“配置保存/退出确认”的行为说明（避免踩坑）。

## 实现里程碑（Milestones）

- [x] M1: 定义并实现 TUI 的 dirty tracking + 退出保存确认（契约优先，含测试）
- [x] M2: 在 deploy 前加入自动保存（含失败阻断与错误提示；含测试）
- [x] M3: 更新运维文档与使用说明（并通过格式化/质量门禁）

## 方案概述（Approach, high-level）

- 在 `src/ops/tui.rs` 引入一个可测试的状态机/基准快照：
  - 启动时记录“基准值”（来自 `load_tui_config` 或默认值）。
  - 每次成功保存后更新基准值。
  - `Ctrl+Q` 时若与基准不同，进入“确认退出”状态，由用户选择保存/丢弃/取消。
- `Ctrl+D` 触发 deploy 时：
  - 先执行与 `Ctrl+S` 等价的保存逻辑（保存 settings；按策略处理 token）。
  - 保存成功才进入 deploy；保存失败回到 TUI 并展示错误。

- 交互改造：
  - 在 NAV/EDIT 两种模式间保持一致的焦点管理（Tab/箭头/鼠标点击）。
  - 命令操作统一改为 Ctrl 组合键；退出确认在 TUI 内完成。
  - 移除 token 的“是否保存”开关：token 与其他字段一样参与 dirty tracking 与保存链路。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：
  - `dry_run=true` 仍会写入 deploy settings，可能与部分用户对 “dry-run 无副作用” 的直觉不一致；需要在 `Help` 与文档中明确说明。
- 开放问题：None
- 假设（需主人确认）：
  - None

## 变更记录（Change log）

- 2026-01-20: 创建计划。
- 2026-01-21: 确认“配置范围”为 deploy settings；确认 `dry_run=true` 时 deploy 前仍自动保存配置。
- 2026-01-21: 确认退出保存确认的快捷键与无默认选项；确认 token 改动计入未保存更改判断。
- 2026-01-21: 新增交互要求：Tab/箭头/鼠标切换焦点与编辑项；命令操作改为 Ctrl 组合键；移除 token 的“是否保存”开关。
- 2026-01-21: 确认 token 输入框为空时保存/自动保存保持现有 token 不变；确认 Ctrl 组合键映射（`Ctrl+S`/`Ctrl+D`/`Ctrl+Q`）。
- 2026-01-21: 落地实现与测试：deploy 前自动保存、dirty tracking + 退出保存确认、Ctrl 组合键命令、鼠标点击切换焦点；同步运维文档。

## 参考（References）

- `src/ops/tui.rs`: Deploy wizard（当前 `d` 不保存、`q/Esc` 直接退出）
- `src/ops/paths.rs`: `/etc/xp-ops/deploy/settings.json` 路径定义
- `src/ops/util.rs`: `write_string_if_changed`/`ensure_dir` 等文件写入工具
