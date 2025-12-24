# xp

轻量的 Xray 管理系统（控制平面），Rust 实现，面向个人自建多节点统一管理。

## 目标（MVP）

- 每台服务器仅运行 `xp` + `xray` 两个进程；`xp` 常驻内存预算 ≤32MiB（RSS，不含 xray）
- 动态创建/管理 Xray 入站与 clients（VLESS+REALITY、SS2022）
- 月度配额（双向合计），超限自动封禁
- 1–20 节点集群，Raft 强一致，无外部依赖
- 订阅输出：URI / Base64 / Clash YAML

## 设计文档

- 统一入口：`docs/desgin/README.md`
- 建议阅读顺序：`docs/desgin/requirements.md` → `docs/desgin/tech-selection.md` → `docs/desgin/architecture.md` → `docs/desgin/quality.md`

## 状态

当前处于设计阶段（未开始代码实现）。

## 开发（初始化脚手架）

工具链（commitlint + dprint）：

```bash
bun install
```

前端：

```bash
cd web
bun install
bun run dev
```

后端：

```bash
cargo run
```

> 默认监听 `127.0.0.1:62416`；如需改端口：`cargo run -- --bind 127.0.0.1:<port>`。

> Git hooks 由 `lefthook.yml` 定义；需要先在本机安装 lefthook 并执行 `lefthook install`。
