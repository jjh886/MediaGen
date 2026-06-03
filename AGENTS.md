# Repository Guidelines

## Project Structure & Module Organization

MediaGen 是一个独立的 Tauri 桌面应用，仓库根目录就是应用根目录。`src/` 保存 React
前端代码，按 `app/`、`features/`、`shared/` 三层组织；`src-tauri/src/` 保存 Rust
运行时、本地 API、调度器、插件执行引擎和存储逻辑；`plugins/` 保存各平台插件
`manifest.json` 与浏览器脚本；`docs/api/` 保存 OpenAI 兼容接口资料；`scripts/` 保存格式、
质量门禁和仓库自动化脚本。测试文件与被测代码同目录，命名为 `*.test.ts(x)`，Rust 测试放在
同文件 `tests` 模块或相邻测试文件中。

## Build, Test, and Development Commands

- `make help`：查看根目录可用命令。
- `make dev` / `npm run tauri:dev`：启动 MediaGen Studio 桌面开发环境。
- `make build` / `npm run tauri:build`：构建桌面端安装包。
- `make check`：运行 TypeScript 类型检查、lint、Vitest 和 Rust 编译检查。
- `make preflight`：提交前完整门禁，包含格式、前端检查、覆盖率、构建和 Rust 检查。
- `npm run format:check`：根目录 Prettier 全量格式检查。
  依赖安装优先使用 `npm ci`；Rust 依赖变更后运行 `cargo check --manifest-path src-tauri/Cargo.toml`。

## Coding Style & Naming Conventions

TypeScript 使用 2 空格缩进、单引号、无分号、`printWidth: 100`，以根目录 `.prettierrc`
为准。React 组件使用 PascalCase，hooks 使用 `useXxx`，测试文件紧邻实现文件。前端禁止
`any`，未使用变量需用 `_` 前缀明确保留。Rust 代码遵循 `cargo fmt` 默认风格，错误类型优先用
`thiserror`，日志使用 `tracing`。

## Architecture Boundaries

React 组件禁止直接调 Tauri command，必须通过 `src/shared/services` 或功能模块 service 层。
`src/shared` 不能反向依赖 `src/features` 或 `src/app`，不同 feature 之间不要互相 import。
Rust 的 `engine/` 只保留原子能力，平台差异优先放在 `plugins/{platform}/manifest.json` 和脚本里，
不要把平台流程硬编码进调度器或通用引擎。

## Testing Guidelines

前端使用 Vitest + Testing Library，运行 `npm run test`。Rust 使用 Cargo test，运行
`npm run cargo-test`。覆盖率门禁在 `scripts/gates/check-coverage-gate.mjs`，核心层阈值：
UI 75%、Rust 75%。新增 service、hook、API 或核心分支时必须补测试。

## Commit & Pull Request Guidelines

提交信息建议使用 Conventional Commits，例如 `feat(api): add openai image response`。提交应说明
用户可见变化或模块范围。PR 需包含变更摘要、测试结果、关联需求或 issue；涉及 UI 时附截图或录屏。
不要提交密钥、`.env`、构建产物、缓存目录或无关格式化改动。除非用户明确要求，禁止自动 commit。

## Security & Configuration Tips

外部输入必须在边界校验；密钥和令牌只放环境变量或本地安全存储。日志记录上下文，不记录敏感数据。
修改认证、上传、浏览器自动化、OpenAI 兼容接口或第三方平台接口时，同时检查失败路径、超时和限流处理。
