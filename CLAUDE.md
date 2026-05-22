## 1. 产品上下文

Frame64 Studio 是一个独立桌面应用，用本地 OpenAI 兼容 API 和浏览器/API 插件驱动图片生成平台。
图片接口直接返回 OpenAI 风格的 base64 JSON，不依赖外部业务系统上传或回写链路。

**核心架构**：React 桌面界面 + Tauri Rust 运行时 + 本地 HTTP API + 薄插件。新平台优先通过
`plugins/{platform}/manifest.json` 和可选 `scripts/` 接入，尽量不改 App 核心代码。

## 2. 架构硬边界

- 桌面端是唯一执行真源，不再保留旧 Web 管理后台和 Go 服务。
- 禁止在 `engine/`、`orchestrator.rs`、`scheduler.rs` 中写平台特定流程。
- 前端组件禁止直接调 Tauri command，必须通过 service 层。
- OpenAI 兼容接口的请求/响应结构以 `docs/api/openai-api.md` 为参考。

| 层             | 允许做                       | 禁止做                  |
| -------------- | ---------------------------- | ----------------------- |
| Rust Engine    | 执行原子操作                 | 写平台特定逻辑          |
| Orchestrator   | 读 manifest、按步骤调 engine | 硬编码平台流程          |
| Scheduler      | 账号轮询、并发、配额         | 直接操作浏览器或发请求  |
| React Frontend | UI 展示、调 service 层       | 直接 invoke、写业务逻辑 |

## 3. 代码风格

**Prettier**：2 空格、单引号、无分号、行宽 100、尾逗号 es5、LF。

**命名**：组件 PascalCase.tsx、hook useXxx、工具 kebab-case.ts、类型 PascalCase、常量
UPPER_SNAKE、Rust snake_case。

**ESLint**：`no-explicit-any` warn、`no-unused-vars` error、`max-warnings` 0、`no-console` warn。

**Rust**：`cargo fmt` 通过。`tracing` 记日志。`let _ =` 只用于明确允许忽略的非关键结果。

## 4. 结构约束

| 指标     | 阈值                           |
| -------- | ------------------------------ |
| 单文件   | **500 行**（350 行评估拆分）   |
| 单函数   | **50 行**                      |
| useState | **5 个**（超过拆组件/提 hook） |

## 5. 前端三层架构

```text
src/
├── app/          # 壳层：路由、布局、Provider
├── features/     # 功能模块（components/ hooks/ services/ types/）
├── shared/       # 共享层（ui/ hooks/ services/ stores/ types/ utils/）
└── styles/       # 全局样式和设计 token
```

**导入边界**：app → features/shared ✓ | features → shared ✓ | 跨 feature ✗ | shared → app/features ✗

## 6. Rust 与插件结构

```text
src-tauri/src/
├── api_server/       # 本地 HTTP API，后续承载 OpenAI 兼容接口
├── engine/           # 原子能力引擎（http/browser/account/email/file/watermark/image）
├── orchestrator.rs   # 流程编排（读 manifest steps → 调 engine）
├── scheduler.rs      # 账号调度、并发和队列
├── plugin_loader.rs  # 加载 plugins 下的平台能力
├── storage/          # SQLite 存储
├── types.rs          # 共享类型
└── error.rs          # thiserror AppError

plugins/
├── GPTImage2/
├── chatgpt/
├── doubao/
├── flow/
└── jimeng/
```

## 7. 关键约定

- **API 兼容**：新接口优先对齐 `docs/api/openai-api.md`，图片生成结果返回 base64。
- **插件薄化**：平台差异尽量放 manifest 和脚本，不把平台判断散落进 Rust 核心。
- **状态管理**：Zustand store 按 feature 划分，组件不直接调底层 API。
- **错误处理**：Rust 用 thiserror + tracing；前端给用户明确可理解的提示。
- **COMPAT 注释**：`// COMPAT(yyyy-MM-dd): 说明`，最长 6 个月。
- **Git**：不自动 commit，等用户 review 后再决定。

## 8. 验证命令

```bash
# 前端
npm run type-check
npm run lint
npm run test
npm run build

# Rust
npm run cargo-check
npm run cargo-test

# 全局门禁
make preflight

# 项目启动
npm run tauri:dev
```

## 9. 文档索引

| 文档                | 路径                     | 内容                        |
| ------------------- | ------------------------ | --------------------------- |
| OpenAI 图片接口参考 | `docs/api/openai-api.md` | 标准接口、base64 响应等参考 |
