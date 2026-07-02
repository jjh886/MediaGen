# Repository Guidelines

## Project Structure & Module Organization

MediaGen 当前只保留新架构：`apps/console/` 保存 React Web Console，按 `app/`、`features/`、
`shared/` 三层组织；`apps/server/` 保存 Rust 服务端、本地 API、调度器、插件执行入口和存储逻辑；
`apps/local-launcher/` 保存 Windows、macOS、Linux 本机图标启动器；`crates/` 保存通用 Rust 原子能力；
`plugins/` 保存各平台插件 `manifest.json` 与浏览器脚本；`docs/api/` 保存 OpenAI 兼容接口资料；
`scripts/` 保存格式、质量门禁、启动脚本和安装包自动化脚本。测试文件与被测代码同目录，命名为
`*.test.ts(x)`，Rust 测试放在同文件 `tests` 模块或相邻测试文件中。

## 架构硬边界

rust只提供原子能力，不处理任何特定平台的规则，所有的特台特有的特性全部放在插件中完成。如果需要功能当前的原子能力不满足，可以新增或扩展当前的原子能力（有限扩展），但是所有的原子能力一定要考虑通用性，不要和任何平台绑定。

## Build, Test, and Development Commands

- `make help`：查看根目录可用命令。
- `make dev` / `npm run dev:console`：启动 Web Console 开发环境。
- `make start-local`：用本机启动脚本启动服务并打开 Web Console。
- `make build` / `npm run build`：构建 Web Console、Rust 服务端和本机启动器。
- `make package-installers`：打包 Windows、macOS、Linux 本机安装产物。
- `make check`：运行 TypeScript 类型检查、lint、Vitest 和 Rust 编译检查。
- `make preflight`：提交前完整门禁，包含格式、前端检查、覆盖率、构建和 Rust 检查。
- `npm run format:check`：根目录 Prettier 全量格式检查。
  依赖安装优先使用 `npm ci`；Rust 依赖变更后运行 `cargo check --workspace`。

## Coding Style & Naming Conventions

TypeScript 使用 2 空格缩进、单引号、无分号、`printWidth: 100`，以根目录 `.prettierrc`
为准。React 组件使用 PascalCase，hooks 使用 `useXxx`，测试文件紧邻实现文件。前端禁止
`any`，未使用变量需用 `_` 前缀明确保留。Rust 代码遵循 `cargo fmt` 默认风格，错误类型优先用
`thiserror`，日志使用 `tracing`。

## Architecture Boundaries

React 组件禁止直接调用服务端接口，必须通过 `apps/console/src/shared/services` 或功能模块 service
层。`apps/console/src/shared` 不能反向依赖 `features` 或 `app`，不同 feature 之间不要互相 import。
Rust 只保留通用原子能力、服务编排和公共协议适配，平台差异优先放在 `plugins/{platform}/manifest.json`
和脚本里，不要把平台流程硬编码进通用 Rust 模块。

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
