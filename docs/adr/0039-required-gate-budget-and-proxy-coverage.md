# 合并门禁的墙钟预算与代理覆盖来源

- Status: accepted

## 背景

合并前检查曾同时承担三种职责：日常质量门禁、性能验收测量、发布证据生产。三者混在同一
触发面上，导致两个具体后果：

- `xray-e2e` 每次 PR 与每次 main push 都运行，实测 12m47s，其中 8m02s 是
  `Managed HTTPS fallback parity` 现场编译 `xp`/`xp-ops`/`xp-proxy` 三个 debug 二进制；
- `ci-performance` 的 `paths` 覆盖 `src/**`、`web/**`、`Cargo.toml`，即每个产品 PR 都触发
  30 分钟测量，并写入约 2.96 GiB、按 run+attempt 命名而永不复用的 Actions 缓存。本 PR 只回收
  这些缓存，**未窄化 `paths`**；那仍是未决项，普通产品 PR 仍会触发该测量。

同时，`ci` 的 `cargo test`/`cargo clippy`/`run-rust-tests.sh` 全部作用于
`workspace_default_members`（仅根包 `xp`），因此 `crates/xp-proxy` 与 `crates/xp-proxy-core`
的单测与 lint 从未进入任何 required 检查。`xp-proxy-perf-gate` 虽然 required，但
`scripts/perf/xp-proxy/policy.py` 的 `related()` 不认 `src/native_proxy_supervisor.rs`，改 XP 侧
代理代码时该门禁以 “not applicable” 通过。

## 决定

1. **只有 Required Gate 受墙钟预算约束**（≤10 分钟，热路径）。验收测量（`ci-performance`、
   `release-performance`）与发布流程（`resource-evidence`、`release`）不受此约束，
   因为 300 秒采样窗口与 3×cold/warm 矩阵在数学上无法压入 10 分钟。
   预算按热路径定义：缓存命中时每个 PR job 应 ≤10 分钟；缓存未命中的冷运行允许 30 分钟，
   因为它只由 `Cargo.lock` 或 toolchain 变更触发。
2. **`xray-e2e` 退出 required，降为手动 + 每周一次**。它验证的是已发布的
   `xray-linux-*` 运行时工件，不是每个 PR 的回归面。
3. **代理覆盖改由独立的 `xp-proxy` job 承担**：`ci` 新增并行 job 运行
   `cargo test -p xp-proxy -p xp-proxy-core --all-targets`，并在同一 job 内运行对应的
   `--all-targets --all-features` clippy。独立 job 而不加进 `rust` job，因为后者实测
   10m24s，已无预算余量。
4. **`xp-proxy-perf` 取代 `xray-e2e` 进入发布就绪名单**，并扩展 `related()` 到
   `src/native_proxy_supervisor.rs` 与 `src/reconcile.rs`，使 XP 侧代理改动触发真实测量。

## 备选方案

- **保留 `xray-e2e` 并加缓存压入 10 分钟**：实测 parity 步骤要靠缓存命中才能降到约 4 分钟，
  冷缓存那一轮仍逼近上限；且它验证的对象（发布工件）与 PR 改动的关联度低。
- **删除 `xray-e2e`**：会失去 `xray-linux-*` 唯一的真实集成验证，而容器部署仍发布该二进制。
- **把验收测量也压进 10 分钟**：等价于放弃 300 秒采样窗口与三次冷/热重复，直接削弱现有
  资源与性能合同。

## 后果

- Required Gate 的预算可执行：常规 PR job 写死 `timeout-minutes: 10`；`rust` job 写死 30，
  因为冷编译加 2160 个 lib 测试实测 10m24s，用 10 分钟会连依赖升级都判红。
- 代理代码进入主干门禁；`xp-proxy-perf-gate` 不再对 XP 侧代理改动返回 not applicable。
- 发布就绪改为等待 `xp-proxy-perf` 的 main push 成功；`xray-e2e` 不再阻塞发布。
- `ci-performance` 的测量缓存必须在断言之后回收，否则会继续挤出 `resource-evidence`
  的构建缓存，使每次证据运行都退化为约 27 分钟的冷编译。回收不改变 PR 触发频率，
  因此本次修复的只是缓存驱逐，不是测量次数。
- Rust 缓存 key 不再包含 `TARGET_SHA`，改为绑定 `Cargo.lock`、toolchain 与 Rust/原型源码 hash。
  `TARGET_SHA` 让每次合并都产生一份永不命中的新条目，这是缓存驱逐的主因；而源码 hash 让
  「命中」等价于「输入字节相同」，避免固定 key 被 `actions/cache` 的「命中即不覆盖」语义
  冻结在旧快照上。Web 源码不再进入 Rust key，因此前端改动不会触发整个依赖图重编译。
  `web`、`storybook`、`docker-smoke` 的 key 未改。
- 冷路径（新 `Cargo.lock` 或 toolchain 变更）不受 10 分钟预算约束，上限为 30 分钟；
  缓存仍会在该次运行结束时保存，后续运行回到热路径。
- 资源证据与发布二进制之间不做 digest 绑定。证据运行以 `--features resource-fixture`
  构建被测候选，发布构建不带该 feature，两者结构上不可能字节相等；为绑定而额外构建一份
  无 feature 候选会把证据运行推过其 45 分钟超时。证据因此只证明源级内存与吞吐，不证明
  发布的这些字节被测过。
