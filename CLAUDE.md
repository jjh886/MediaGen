## 1. 产品上下文

MediaGen 是一个插件化的多平台生成任务调度系统，对外提供 OpenAI 兼容图片接口，对内通过
CloakBrowser 和平台插件完成网页侧生成、下载、登录授权和人机验证接管。

当前项目只保留新架构：一套 Web Console、一套 Rust Server、一个本机图标启动器。Windows、macOS
和 Linux 本机用户点击图标后由 `apps/local-launcher` 启动服务并打开网页；服务器用户直接访问
`apps/server` 提供的 Console 地址。

## 2. 架构硬边界

- Rust 只提供通用原子能力、服务编排和公共协议适配，不写平台专属流程。
- 平台页面规则、DOM 识别、网络轮询、结果解析必须放在 `plugins/{platform}/manifest.json` 和脚本中。
- React 组件不能直接调用 HTTP API，必须通过 `apps/console/src/shared/services` 或 feature service 层。
- OpenAI 兼容接口的请求/响应结构以 `docs/api/openai-api.md` 为参考。

| 层              | 允许做                                | 禁止做                 |
| --------------- | ------------------------------------- | ---------------------- |
| Browser Gateway | CloakBrowser、CDP、画面、输入原子能力 | 写平台登录和生成规则   |
| Server          | 管理 API、调度、账号、任务、插件执行  | 硬编码平台网页流程     |
| Plugin Runtime  | 读取 manifest、执行脚本               | 把通用能力复制到插件内 |
| Web Console     | UI 展示、调用 service 层              | 直接写平台或后端细节   |
| Local Launcher  | 启动服务、打开网页                    | 处理账号和插件业务     |

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
apps/console/src/
├── app/          # 壳层：路由、布局、Provider
├── features/     # 功能模块（components/ hooks/ services/ types/）
├── shared/       # 共享层（ui/ hooks/ services/ stores/ types/ utils/）
└── styles/       # 全局样式和设计 token
```

导入边界：app -> features/shared，features -> shared；禁止跨 feature 互相 import；禁止 shared 反向依赖
app/features。

## 6. Rust 与插件结构

```text
apps/server/                       # 管理 API、OpenAI 兼容 API、调度和插件执行入口
apps/local-launcher/               # 本机图标启动器
crates/mediagen-browser-gateway/   # CloakBrowser + CDP 通用原子能力
crates/mediagen-core/              # 通用类型
crates/mediagen-plugin-runtime/    # 插件加载和 JS flow 执行

plugins/
├── _shared/
├── chatgpt/
└── codex/
```

## 7. 关键约定

- **API 兼容**：新接口优先对齐 `docs/api/openai-api.md`，图片生成结果返回 base64。
- **插件薄化**：平台差异尽量放 manifest 和脚本，不把平台判断散落进 Rust 核心。
- **账号登录**：添加账号、重新登录、人机验证统一使用 Browser Session 和 Console 内网页渲染。
- **状态管理**：Zustand store 按 feature 划分，组件不直接调底层 API。
- **错误处理**：Rust 用 thiserror + tracing；前端给用户明确可理解的提示。
- **Git**：不自动 commit，等用户 review 后再决定。

## 8. 验证命令

```bash
make check
```

## 9. 启动命令

```bash
make dev  # 同时启动 Rust 服务端和 Web Console
```

## 10. 文档索引

| 文档                | 路径                                      | 内容                        |
| ------------------- | ----------------------------------------- | --------------------------- |
| OpenAI 图片接口参考 | `docs/api/openai-api.md`                  | 标准接口、base64 响应等参考 |
| 服务端化开发规格    | `docs/mediagen-standard-monorepo-plan.md` | 当前项目结构和运行逻辑      |
