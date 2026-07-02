<div align="center">

<img src="scripts/start/assets/mediagen.png" alt="MediaGen" width="120" />

# MediaGen

**把网页里的图片生成功能变成标准 OpenAI 兼容 API**

支持多账号接入 · 支持本机图标启动 · 支持服务器部署 · 基于 CloakBrowser 执行网页插件

<p>
  <img alt="React" src="https://img.shields.io/badge/React-19-61DAFB?logo=react&logoColor=white" />
  <img alt="Rust" src="https://img.shields.io/badge/Rust-stable-000000?logo=rust&logoColor=white" />
  <img alt="Platform" src="https://img.shields.io/badge/Platform-macOS%20%7C%20Windows%20%7C%20Linux-555" />
</p>

</div>

---

## 这是什么？

很多 AI 平台只能在网页里生成图片，没有稳定 API，或者 API 能力不完整。

MediaGen 的目标是把这些网页能力封装成本机或服务器上的统一接口：你像调用 OpenAI 一样发请求，MediaGen 会选择可用账号、打开平台网页、执行插件流程，并把图片结果返回给调用方。

## 快速开始

本机开发：

```bash
npm ci
make dev-server
make dev
```

本机普通用户启动：

```bash
make start-local
```

构建本机安装产物：

```bash
make package-installers
```

构建全部核心产物：

```bash
make build
```

## 工作方式

```text
OpenAI SDK / curl
  -> apps/server: /v1/images/* 和管理 API
  -> 插件调度: 选择平台、账号、代理和执行步骤
  -> mediagen-browser-gateway: 启动 CloakBrowser 并连接 CDP
  -> apps/console: 渲染账号登录、人机验证和后台管理界面
  -> 返回 base64 图片结果
```

本机用户点击 MediaGen 图标时，`apps/local-launcher` 只负责启动本机 `mediagen-server` 并打开浏览器访问 Web Console。所有账号、任务、插件、API Key 和浏览器会话逻辑都在 `apps/server` 与 `apps/console` 中完成。

服务器用户直接部署 `mediagen-server`，再通过浏览器访问 Web Console。没有桌面环境的 Linux 服务器也走同一套页面和同一套服务端能力。

## OpenAI 兼容 API

默认地址：

```text
http://127.0.0.1:18080
```

查看模型：

```bash
curl http://127.0.0.1:18080/v1/models
```

生成图片：

```bash
curl http://127.0.0.1:18080/v1/images/generations \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "gpt-image-1",
    "prompt": "A clean product photo of a glass water bottle on a white desk",
    "size": "1024x1024",
    "quality": "high"
  }'
```

在 OpenAI SDK 中使用：

```ts
import OpenAI from 'openai'

const client = new OpenAI({
  apiKey: 'mediagen-local',
  baseURL: 'http://127.0.0.1:18080/v1',
})

const result = await client.images.generate({
  model: 'gpt-image-1',
  prompt: 'A minimal desk setup with a silver laptop and soft daylight',
  size: '1024x1024',
})

const imageBase64 = result.data?.[0]?.b64_json
```

## 账号和浏览器登录

添加账号、重新登录和人机验证都使用统一的 Browser Session：

```text
Console 创建 BrowserSession
  -> server 创建会话
  -> browser-gateway 启动 CloakBrowser
  -> Console 直接渲染网页内容
  -> 用户完成登录或验证
  -> 插件检测登录成功
  -> server 保存 profile / cookies 并关闭会话
```

每个账号可以配置独立代理，当前服务端代理地址优先使用 `socks5://...` 格式。账号 profile、cookies、代理配置和 API Key 都由服务端统一管理。

## 插件机制

每个平台插件放在 `plugins/{platform}/`：

```text
plugins/{platform}/
├── manifest.json
└── scripts/
```

核心原则：

- 平台页面规则、DOM 识别、网络轮询和结果解析放在插件里。
- Rust 只提供通用原子能力、服务编排和公共协议适配。
- Web Console 只通过 service 层调用服务端 API，不直接写平台专属逻辑。

## 常用命令

| 命令                      | 说明                             |
| ------------------------- | -------------------------------- |
| `make dev`                | 启动 Web Console 开发环境        |
| `make dev-server`         | 启动 Rust 服务端                 |
| `make dev-launcher`       | 启动本机图标启动器开发版         |
| `make start-local`        | 用脚本启动本机服务并打开 Console |
| `make build`              | 构建 Console、Server、Launcher   |
| `make package-local`      | 打包本机发行目录                 |
| `make package-installers` | 打包本机可安装产物               |
| `make check`              | 运行常用检查                     |
| `make preflight`          | 提交前完整质量门禁               |

## 项目结构

```text
.
├── apps/
│   ├── console/          # React Web Console
│   ├── server/           # Rust 服务端和 OpenAI 兼容 API
│   └── local-launcher/   # Windows / macOS / Linux 本机图标启动器
├── crates/
│   ├── mediagen-browser-gateway/
│   ├── mediagen-core/
│   └── mediagen-plugin-runtime/
├── packages/
│   ├── contracts/
│   ├── feature-kits/
│   └── ui/
├── plugins/
│   ├── _shared/
│   ├── chatgpt/
│   ├── codex/
│   ├── doubao/
│   ├── flow/
│   └── jimeng/
├── deploy/
├── docs/
└── scripts/
```

## 开发约定

- 前端界面只做一套：`apps/console`。
- 本机和服务器复用同一套 Console 与 Server。
- React 组件不能直接调用 HTTP API，必须通过 service 层。
- Rust 不能写平台专属流程；平台差异放到插件 manifest 和脚本。
- 单个文件超过 500 行前必须拆分。
- 新增核心 service、hook、API 或重要分支时需要补测试。
- 不提交密钥、缓存目录、构建产物、平台登录 profile 或 cookies。

## 文档

- [OpenAI 图片接口参考](docs/api/openai-api.md)
- [图片生成接口](docs/api/create-image.md)
- [图片编辑接口](docs/api/edits-image.md)
- [图片变体接口](docs/api/image-variation.md)
- [服务端化开发规格](docs/mediagen-standard-monorepo-plan.md)

## License

正式公开前请补充 `LICENSE` 文件。
