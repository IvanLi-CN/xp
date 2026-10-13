# Mihomo 外部资源镜像主题历史

## Lifecycle / Compatibility

- 本主题收拢已有 `external_resources=mirror` 行为，不改变参数名、默认直连、资源地址格式或 profile 存储。
- 保持 HMAC 资源 ID 和公开 bearer 地址语义，不引入订阅 token 绑定或短期签名。
- 私网目标遵守节点本地 CIDR 契约；旧集群级 policy API 仅保留 GET=false / PUT=410 的兼容行为。

## Replacements / Background

- 外部资源镜像从 Mihomo provider 主题中提取为独立长期边界；provider 拓扑与主配置合同仍由原主题持有。
- ADR 0026 记录公开资源地址、目标目录准入和无缓存代取之间的取舍。
- 原配置中的查询参数仍参与上游请求；镜像资源地址共享即意味着共享相应内容的访问权。

## Related Changes

- 本主题建立时未新增运行时行为；现有源码覆盖与验收缺口记录于 `IMPLEMENTATION.md`。

## References

- [长期需求](./SPEC.md)
- [实现覆盖](./IMPLEMENTATION.md)
- [原 provider 主题](../mihomo-provider-dual-track/SPEC.md)
