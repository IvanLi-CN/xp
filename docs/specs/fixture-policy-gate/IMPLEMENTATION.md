# Fixture 数据策略门禁实现

## 组成

- 根 Bun lockfile 固定 `@ast-grep/cli`；本地与 CI 共用 `bun run check:fixture-policy`。
- `fixture-policy/catalog.json` 是操作型 synthetic fixture 值的唯一来源。TypeScript adapter
  仅静态导入该文件；Rust adapter 是未发布的 path dev crate，且只作为 dev dependency。
- policy 规则测试覆盖 adapter 引用、未批准字面量、动态/外部输入、catalog 类型漂移和
  suppression。
- `.github/CODEOWNERS` 覆盖安全边界文件；远端 required-check 与 review 设置由仓库所有者
  执行。

## 覆盖路径

- Web：所有 `web/src/**/*.test.{ts,tsx}`、`*.spec.{ts,tsx}`、`*.stories.{ts,tsx}`、
  `web/tests/**/*.{ts,tsx}`、`web/.storybook/mocks/**/*.{ts,tsx}`、
  `web/src/storybook/**/*.{ts,tsx}` 与 `web/src/demo/fixtures.ts`。
- Rust：`tests/**/*.rs`、`src/**/*_tests.rs`、`src/**/tests.rs`、`src/**/tests/**/*.rs`、
  `src/**/test_fixtures.rs`，以及 `src/**/*.rs` 内的 `mod ...tests` 单元测试模块。

## 边界

Gate 是声明文本源的静态约束，不覆盖二进制附件、Git 历史或运行时数据流。受保护路径和
adapter 均不得使用 `ast-grep-ignore`；新增操作型 fixture 值必须先进入 catalog，再暴露为
零参数 adapter accessor。

## Related Changes

- `fixture-policy/`
- `web/src/fixture-policy/catalog.ts`
- `test-fixtures/`

## References

- `./SPEC.md`
- `./HISTORY.md`
