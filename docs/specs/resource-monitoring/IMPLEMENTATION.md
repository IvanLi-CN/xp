# Resource Monitoring implementation

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实。

## 当前状态

尚未开始实现。现有 uptime、运行时状态、流量、TCP 连接数与 History Repository 不构成 Resource Monitoring 的实现覆盖。

## 已冻结的合同

- Resource Domain、指标集合、采样频率、容量护栏、专用 history reducer、policy、API 和 UI 均由 `./SPEC.md` 与
  `./contracts/api.md` 定义。

## 未覆盖的实现面

- 运行时采样、SQLite Resource Store、History Repository schema/reducer、alert 扩展、API、Web 视图和部署保留合同均未实现。

## 实施顺序

1. 完成 Linux 数据源、固定角色解析、15 秒采样器、内存窗口与分钟 Rollup。
2. 完成 Resource Store、Source Delivery Journal 接入、专用 history query/reducer 和 quota preflight。
3. 完成 Raft Resource Alert Policy、alert aggregation、Web 总览/详情、部署文档和视觉证据。

## Remaining gaps

- 当前没有可用于 CPU、内存、磁盘、I/O 或 per-role 资源数据的生产 API、持久化或 UI。
- 首版不拥有读取受限 PSS/I/O 的 root 权限；这些字段必须通过 capability 降级。
