<div align="center">

<img src="src-tauri/icons/icon.png" alt="MediaGen Studio" width="120" />

# MediaGen Studio

**把"网站上的图片生成功能"变成"标准 API 接口"的桌面软件**

支持多账号接入 · 兼容 OpenAI Image API · 结果直出 Base64 · 数据全程留在本机

<p>
  <img alt="Tauri" src="https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white" />
  <img alt="React" src="https://img.shields.io/badge/React-19-61DAFB?logo=react&logoColor=white" />
  <img alt="Rust" src="https://img.shields.io/badge/Rust-stable-000000?logo=rust&logoColor=white" />
  <img alt="Platform" src="https://img.shields.io/badge/Platform-macOS%20%7C%20Windows%20%7C%20Linux-555" />
</p>

</div>

---

## 这是什么？

很多 AI 平台（chatgpt，即梦、豆包、Flow 等）只能在网页里点点点生成图片，没有官方 API，也不方便程序调用。

**MediaGen 帮你把这些网站封装成本地接口**：你像调用 OpenAI 一样发一个请求，它会自动用你的账号在对应平台生成图片，再把结果直接返回给你。

> 简单说：一个本地桌面应用，对外提供统一的 OpenAI 风格接口，背后用你的多个平台账号干活。

### 适合谁用

- 想批量、自动化生成图片，但平台没开放 API 的人。
- 已经在用 OpenAI SDK，希望换个图片来源却不想改代码的人。
- 手里有多个平台账号，想统一管理、轮流调用、控制配额的人。

### 主要特点

| 特点                   | 说明                                                           |
| ---------------------- | -------------------------------------------------------------- |
| **像 OpenAI 一样调用** | 兼容 OpenAI Image API，已有代码改个地址就能用                  |
| **多账号接入**         | 桌面端添加多个平台账号，自动轮询、并发和限额，避免单账号被限制 |
| **结果直出 Base64**    | 图片以 `base64` 返回，不上传任何外部服务器，数据留在本机       |
| **多平台插件化**       | 通过插件接入不同网站，新增平台基本不用改核心代码               |
| **本地任务记录**       | 生成任务、日志、账号状态都保存在本机 SQLite 数据库             |

> 当前版本重点是**图片生成**与**图片编辑**。视频生成会继续按同一套插件架构扩展。

---

## 目录

- [快速开始](#快速开始)
- [工作原理](#工作原理)
- [OpenAI 兼容 API](#openai-兼容-api)
- [接口注意事项](#接口注意事项)
- [插件机制](#插件机制)
- [开发](#开发)
- [技术栈](#技术栈)
- [项目结构](#项目结构)
- [文档](#文档)
- [License](#license)

---

## 快速开始

**1. 准备环境**

- macOS、Windows 或 Linux 桌面环境
- [Node.js](https://nodejs.org/) 20+
- [Rust](https://www.rust-lang.org/) stable
- [Google Chrome](https://www.google.com/chrome/)
- Tauri 2 所需系统依赖

**2. 安装依赖并启动**

```bash
npm ci
npm run tauri:dev   # 或 make dev
```

**3. 在桌面界面里添加平台账号**

账号可用后，就能通过本地 API 调用生成图片：

```bash
curl http://127.0.0.1:18080/v1/images/generations \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "gpt-image-1",
    "prompt": "A clean product photo of a glass water bottle on a white desk"
  }'
```

返回的 `data[].b64_json` 就是生成好的图片。完整接口见 [OpenAI 兼容 API](#openai-兼容-api)。

---

## 工作原理

```text
你的程序 ──OpenAI 风格请求──▶ MediaGen 本地服务 (127.0.0.1:18080)
                                      │
                                      ├─ 调度器：挑选可用账号、控制并发与配额
                                      ├─ 编排器：按插件 manifest 步骤执行
                                      └─ 浏览器自动化：在平台网页完成生成 / 下载
                                      │
你的程序 ◀──base64 图片结果──────────┘
```

- **桌面界面（Tauri + React）**：管理平台账号、登录状态、任务和日志。
- **本地服务（Rust）**：负责 OpenAI 兼容 API、账号调度、插件编排和浏览器自动化。
- **数据存储（SQLite）**：账号、任务、设置、日志都存在本机。

---

## OpenAI 兼容 API

默认本地 API 地址：`http://127.0.0.1:18080`（端口可在桌面端设置页面调整）。

### 查看模型

```bash
curl http://127.0.0.1:18080/v1/models
```

<details>
<summary>返回结构示例</summary>

```json
{
  "object": "list",
  "data": [
    {
      "id": "flow_Nano_Banana_Pro",
      "object": "model",
      "created": 0,
      "owned_by": "flow",
      "display_name": "Nano Banana Pro",
      "task_kind": "image"
    }
  ]
}
```

</details>

### 文字生成图片

```bash
curl http://127.0.0.1:18080/v1/images/generations \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "gpt-image-1",
    "prompt": "A clean product photo of a glass water bottle on a white desk",
    "size": "1024x1024",
    "quality": "high",
    "output_format": "png"
  }'
```

<details>
<summary>返回结构示例</summary>

```json
{
  "created": 0,
  "data": [
    {
      "b64_json": "..."
    }
  ],
  "output_format": "png",
  "quality": "high",
  "size": "1024x1024"
}
```

</details>

### 根据参考图编辑

编辑接口兼容 OpenAI SDK 默认的 `multipart/form-data` 文件上传，也支持 JSON 里的
`images[].image_url`（可以是公网图片 URL，也可以是 `data:image/png;base64,...` 这种 data URL）。

**OpenAI SDK / curl 文件上传：**

```bash
curl http://127.0.0.1:18080/v1/images/edits \
  -H 'Authorization: Bearer mediagen-local' \
  -F 'model=gpt-image-1' \
  -F 'prompt=Turn this image into a clean studio product shot' \
  -F 'image=@source.png' \
  -F 'size=1024x1024'
```

**JSON 调用：**

```bash
curl http://127.0.0.1:18080/v1/images/edits \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "gpt-image-1",
    "prompt": "Turn this image into a clean studio product shot",
    "images": [{ "image_url": "https://example.com/source.png" }],
    "size": "1024x1024",
    "quality": "high",
    "output_format": "png"
  }'
```

### 在 OpenAI SDK 中使用

把 SDK 的 `baseURL` 指向 MediaGen 本地服务即可：

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

<details>
<summary>流式调用（SSE）</summary>

流式调用会返回 OpenAI Image API 的 SSE 事件；当前插件只能在最终完成时发送
`image_generation.completed` 或 `image_edit.completed`，不会伪造中间预览图。

```ts
const stream = await client.images.generate({
  model: 'gpt-image-1',
  prompt: 'A clean icon of a camera and play button',
  stream: true,
  partial_images: 0,
})

for await (const event of stream) {
  if (event.type === 'image_generation.completed') {
    const imageBase64 = event.b64_json
  }
}
```

</details>

---

## 接口注意事项

- `/v1/models` 会返回插件真实模型，也会返回 `gpt-image-2`、`gpt-image-1.5`、`gpt-image-1`、
  `gpt-image-1-mini`、`chatgpt-image-latest` 这类 OpenAI 兼容别名。
- MediaGen 只返回 `b64_json`，不返回远程图片 URL。
- `stream: true` 会返回官方 Image API 风格的 SSE；受当前插件能力限制，只发送最终 `completed` 事件。
- 当前不提供 OpenAI Files API，所以 `file_id` 不可用；编辑图片请直接上传 `image` 文件，或使用
  `images[].image_url`。
- `n` 会被限制在 `1..4`，避免单次请求占用过多浏览器和平台资源。
- `size` 支持 `auto`、`1024x1024`、`1536x1024`、`1024x1536`。
- `/v1/images/variations` 当前返回不支持，后续可按插件能力扩展。

---

## 插件机制

每个平台插件通常包含：

```text
plugins/{platform}/
├── manifest.json      # 平台、模型、账号配额、执行步骤
└── scripts/           # 页面检测、输入、轮询、下载等脚本
```

核心原则是 **"插件薄、核心稳"**：

- 平台页面怎么点、接口怎么轮询，优先放在插件里。
- Rust `engine/` 只负责浏览器、HTTP、文件、账号等通用原子能力。
- `orchestrator.rs` 只读取 manifest 步骤并按顺序执行，不写平台专属流程。

---

## 开发

### 构建应用

```bash
npm run tauri:build   # 或 make build
```

构建产物会由 Tauri 输出到 `src-tauri/target/` 下对应平台目录。

### 常用命令

| 命令                   | 说明                 |
| ---------------------- | -------------------- |
| `npm run tauri:dev`    | 启动桌面开发环境     |
| `npm run tauri:build`  | 构建桌面应用         |
| `npm run type-check`   | TypeScript 类型检查  |
| `npm run lint`         | 前端 lint 和结构检查 |
| `npm run test`         | 前端单元测试         |
| `npm run cargo-check`  | Rust 编译检查        |
| `npm run cargo-test`   | Rust 测试            |
| `npm run format:check` | 检查 Prettier 格式   |
| `npm run check`        | 常用检查组合         |
| `make preflight`       | 提交前完整质量门禁   |

### 开发约定

- React 组件不要直接调用 Tauri command，统一走 service 层。
- `src/shared` 不能反向依赖 `src/features` 或 `src/app`。
- 单文件尽量保持在 500 行以内，复杂页面拆成组件、hooks、services。
- 新增核心 service、hook、API 或重要分支时，需要补测试。
- 不提交 `.env`、密钥、缓存目录、构建产物或平台登录数据。

---

## 技术栈

| 模块     | 技术                       | 作用                         |
| -------- | -------------------------- | ---------------------------- |
| 桌面壳   | Tauri 2                    | 打包桌面应用、连接 Rust 能力 |
| 前端界面 | React 19 + Vite + Zustand  | 账号、任务、日志、设置页面   |
| 本地服务 | Rust + Actix Web           | OpenAI 兼容 API、本地队列    |
| 自动化   | Chrome DevTools Protocol   | 登录、生成、下载、校验结果   |
| 插件     | JSON Manifest + JavaScript | 描述不同平台的执行流程       |
| 存储     | SQLite                     | 账号、任务、设置、日志       |

---

## 项目结构

```text
.
├── src/                 # React 桌面界面
│   ├── app/             # 应用壳层和布局
│   ├── features/        # 账号、任务、日志、设置等功能模块
│   └── shared/          # 公共 UI、service、类型
├── src-tauri/           # Tauri + Rust 运行时
│   ├── src/api_server/  # 本地 OpenAI 兼容 API
│   ├── src/engine/      # 浏览器、HTTP、文件、账号等原子能力
│   ├── src/storage/     # SQLite 存储
│   └── icons/           # App 图标
├── plugins/             # 平台插件和浏览器脚本
├── docs/api/            # OpenAI 图片接口参考文档
├── scripts/             # 格式、检查、质量门禁脚本
└── quality/             # Semgrep 等质量规则
```

---

## 文档

- [OpenAI 图片接口参考](docs/api/openai-api.md)
- [图片生成接口](docs/api/create-image.md)
- [图片编辑接口](docs/api/edits-image.md)
- [图片变体接口](docs/api/image-variation.md)

---

## License

当前仓库还没有声明开源许可证。正式公开前，请先补充 `LICENSE` 文件。
