# UI Components

## ServiceConfigPage

- 范围（Scope）: internal
- 变更（Change）: New
- 用途: 只读展示服务配置；展示加载/错误/空态。

### 视图模型（View model）

```ts
export type ServiceConfigView = {
  network: {
    bind: string;
    xray_api_addr: string;
    api_base_url: string;
  };
  node: {
    node_name: string;
    access_host: string;
    data_dir: string;
  };
  quota: {
    quota_poll_interval_secs: number;
    quota_auto_unban: boolean;
  };
  security: {
    admin_token_present: boolean;
    admin_token_masked: string;
  };
};
```

### 行为约定（Behavior）

- 加载态：使用全页 `PageState`（loading）。
- 错误态：展示 `PageState`（error）并提供“重试”。
- 空值展示：字段为空时显示 `"(empty)"`。
