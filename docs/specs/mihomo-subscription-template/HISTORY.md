# History

- The topic originated in legacy Spec `#rbt5e`.
- It preserves the completed user-template and provider-only responsibilities.

## 变更记录（Change log）

- 2026-10-09: `🤯 All` 先列实际 `🛬 {base}`；系统 `🛣️` 使用其他已订阅节点
  `*-reality`，排除目标、未订阅节点和 `DIRECT`；无候选时 `REJECT`。
- 2026-03-04: 创建规格并冻结初版 API/渲染/回退口径。
- 2026-03-04: 完成首版实现与验证（`cargo test`、`web lint/typecheck/test`），PR #95。
- 2026-03-06: 需求升级为“mixin + 系统内置动态组逻辑”，新增稳定入口组、落地组策略与 autosplit 防误用机制。
- 2026-03-06: 对外主字段切换为 `mixin_yaml`，稳定地区范围锁定为 JP/HK/KR。
- 2026-03-08: 将 SS 链式代理收敛为单一 `-chain` 节点，并把系统外层候选组改为 `🛣️ JP/HK/TW`；旧地区组名与 `-JP/-HK/-KR/-TW` 不再兼容映射，旧系统组定义会在渲染阶段被剔除，其引用仅做悬挂引用裁剪。
- 2026-03-06: 在 `codex-testbox` 生成示例输出并完成两类证据：原样例的脱敏输出/差异分析，以及去掉已脱敏静态节点后的 provider-only 变体 Mihomo `-t` 通过记录。
- 2026-03-07: 清理对外 `template_yaml` 兼容层；管理 API、前端 schema/mock 与文档统一只保留 `mixin_yaml`，同时恢复内部状态/WAL 的读写兼容（内部双写）以保证滚动升级安全。
- 2026-03-06: review 收口补充 extra proxy 引用保护：对显式 extra proxies 的名称保持最高优先级，即便名称带有 `-JP` / `-HK` / `-KR` / `-ss` / `-reality` 后缀，也不再被系统动态 remap 误绑。
- 2026-04-24: 将系统托管地区组切换为“节点主动探测出口 IP + Geo 解析”唯一真相源；新增固定 `Japan/HongKong/Taiwan/Korea/Singapore/US/Other` 地区面，以及高质量/节点选择聚合组自动补点。
- 2026-06-19: 管理端与文档收口到 raw profile storage 合同；前端与 mock 不再假设会对 `mixin_yaml.proxies` / `mixin_yaml.proxy-providers` 做本地或服务端自动抽取。
- 2026-10-06: 收敛 Mihomo 全视口编辑需求并纳入本主题；固定三份文档、整份 Profile 保存、草稿保护与窄屏文件抽屉。
- 2026-10-07: 将全视口工作区收敛为连续的多文件编辑器布局，补齐桌面/窄屏视觉证据，并通过 Storybook、Demo 与 User Details 浏览器验证。
- 2026-10-07: 根据 Tier 3 收敛补齐同一用户会话与查询代际保护、详情直链离页保护，以及选区/滚动/撤销重做、只读、保存中和失败重试的交互证据；当前候选为 PR #386。
- 2026-10-07: 补齐生产与 Demo 的应用导航、登出和删除离页守卫，处理保存进行中的无效丢弃路径，并以真实三文件保存回读、应用内路由切换和 Impeccable 扫描完成最终候选验证。
- 2026-10-10: 修复 CodeMirror 全部折叠快捷键只创建顶层范围的问题；现在展开父对象后仍保留子对象折叠，并补充真实 Demo Mihomo 多层配置视觉证据。
