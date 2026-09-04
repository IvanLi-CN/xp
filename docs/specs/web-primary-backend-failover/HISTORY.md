# Web 主后端切换演进历史

> 长期行为合同见 `./SPEC.md`；此处仅保留主题生命周期和兼容性背景。

## Decision Trace

- 采用嵌入式 PWA 的单主后端手动切换，暂不引入独立静态部署。
- 采用当前注册节点的精确 origin allowlist，避免任意 URL 和跨集群 token 泄露。
- 保留既有全页节点导航作为兼容路径，不将其改造成浏览器直连多节点控制面。

## Compatibility

- API 兼容窗口与 PWA build ID 继续独立。
- 已有相对 API 调用、SSE、离线读缓存和 mutation 写保护继续有效。
