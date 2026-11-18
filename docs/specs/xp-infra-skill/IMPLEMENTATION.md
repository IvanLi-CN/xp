# 实现映射

- Skill、只读 CLI、探针、测试与公开模板：`skills/xp-infra/`。
- 发布和安装：项目发布工作流与版本化 Skill 包；私人配置保留在仓库外。
- Linux 探针与打包测试由 GitHub Actions 执行；真实节点结果单独保存在私人运维记录中。
- `listener.test.ts` 验证 runtime 的公网监听与 PID 绑定，后续冲突不得保留先前成功结果。
- `remote.test.ts` 在 Linux 执行宿主与容器身份复核及 HTTP/1.1、HTTP/2 回落探针。
- 发布合同测试检查 `package-skill` 依赖、锁定源码 SHA、XP 版本和同一 run 的资产收集。
