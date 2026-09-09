# Codex 模型路由（自用构建）

在「设置 → 代理」的「Codex 模型路由」面板配置规则，然后开启 Codex 本地路由。规则按从上到下顺序匹配，第一条命中的规则决定目标供应商。

| 匹配方式 | 匹配值 | 目标供应商 |
| --- | --- | --- |
| 前缀匹配 | `gpt-` | 你已有的 GPT 供应商 |
| 前缀匹配 | `grok-` | 你已有的 Grok 供应商 |

前缀匹配不使用 `*`。供应商关联保存在 SQLite settings 的 `codex_model_routing_v1` 项中，使用供应商 ID；名称、地址和密钥都从供应商卡读取。

未命中规则时沿用默认供应商和原有故障转移设置。命中规则时失败直接返回，不切换到别家，也不会切换 CCS 当前供应商。禁用或删除规则后从下一次请求生效，进行中的请求保持原目标。官方 OAuth 账号卡不支持作为模型路由目标。

模型列表来自供应商卡中的模型目录。没有 GPT 目录时，导入本机 Codex 的缓存或内置模型信息；这些信息表示客户端能力，不保证你的上游账户拥有所有模型的访问权。需要限定列表时，请先在供应商卡配置对应模型。使用预览查看实际合并列表及缺失目录的供应商。

合并目录沿用 CCS 管理的 `cc-switch-model-catalog.json`。目录更改后需要重启 Codex，规则本身可以热更新。用户自建目录不会被覆盖。路由模式由各模型元数据提供上下文窗口，不继承默认供应商的统一上下文/压缩阈值；关闭模型路由后恢复默认供应商配置。

自用构建关闭了官方升级检查和安装入口，更新需自行同步 fork 并重新构建。保留原 MIT 许可证。

## 构建与验证

```sh
pnpm install --frozen-lockfile
pnpm typecheck
pnpm exec vitest run tests/components/ModelRoutingPanel.test.tsx
cargo test --manifest-path src-tauri/Cargo.toml --lib model_routing
pnpm tauri build --bundles app
```

Apple Silicon 上默认构建 arm64，产物为 `src-tauri/target/release/bundle/macos/CC Switch.app`。无需生成官方自动更新签名。

本机自用产物在打包后补充 ad-hoc 签名并验证（不等同于 Apple 公证）：

```sh
codesign --force --deep --sign - 'src-tauri/target/release/bundle/macos/CC Switch.app'
codesign --verify --deep --strict --verbose=2 'src-tauri/target/release/bundle/macos/CC Switch.app'
```

## 本次验证记录（2026-09-09）

- Rust 定向测试 18 项通过：模型路由与目录恢复 5 项、原有供应商路由 8 项、Codex 恢复 4 项、恢复后切换供应商 1 项。
- 前端定向测试 4 项通过，覆盖模型路由面板和代理页；TypeScript 类型检查及变更文件格式检查通过。
- 本机 Codex 成功解析合并目录的 13 个模型，包含 3 个 Grok 模型；该检查未调用真实上游。
- Apple Silicon release `.app` 构建成功，arm64 架构及本地签名验证通过。
- 尚未验证安装后原生界面中的真实 GPT/Grok 推理切换；现有 CCS 安装、数据库及 Codex 配置未替换。测试使用内存数据库、临时配置及模拟上游，不能代替真实账户的端到端验收。

首次安装前，退出旧 CCS 后备份整个 `.cc-switch` 目录（包含数据库 WAL）、Codex 的 `config.toml` 和 CCS 管理的模型目录；不要同时运行两个实例使用同一份数据。保留原 `.app` 可回退。验证采用临时目录、内存数据库及两个本机模拟上游，测试不需要真实 API Key。
