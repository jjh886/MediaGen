# Frame64 Studio

<p align="center">
  <img src="src-tauri/icons/icon.png" alt="Frame64 Studio" width="120" />
</p>

Frame64 Studio 是一个独立的桌面端图片生成应用。它用 Tauri + React 做桌面界面，用 Rust
负责本地 HTTP API、账号调度、插件编排和浏览器自动化。外部程序可以按 OpenAI Image API 的习惯调用
Frame64，本地生成完成后直接拿到 `base64` 图片数据，不需要再上传到其它业务系统。

简单理解：Frame64 把多个图片生成平台封装成一个本地桌面应用，并对外提供统一的 OpenAI 风格接口。

## 当前能力

- 独立桌面应用：不依赖旧前后端项目，也不需要旧业务系统。
- OpenAI 风格接口：支持 `/v1/models`、`/v1/images/generations`、`/v1/images/edits`。
- Base64 直出：生成结果放在 `data[].b64_json`，不会上传到外部存储。
- 多平台插件：平台差异放在 `plugins/`，核心代码尽量保持通用。
- 账号与配额管理：桌面端管理平台账号、登录状态、并发和每日用量。
- 本地任务记录：生成任务、日志、账号状态都保存在本机 SQLite 数据库中。

> 当前版本重点是图片生成与图片编辑。视频生成会继续按同一套插件架构扩展。

## 技术栈

| 模块     | 技术                       | 作用                         |
| -------- | -------------------------- | ---------------------------- |
| 桌面壳   | Tauri 2                    | 打包桌面应用、连接 Rust 能力 |
| 前端界面 | React 19 + Vite + Zustand  | 账号、任务、日志、设置页面   |
| 本地服务 | Rust + Actix Web           | OpenAI 兼容 API、本地队列    |
| 自动化   | Chrome DevTools Protocol   | 登录、生成、下载、校验结果   |
| 插件     | JSON Manifest + JavaScript | 描述不同平台的执行流程       |
| 存储     | SQLite                     | 账号、任务、设置、日志       |

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

## 环境要求

开发前请先准备：

- macOS、Windows 或 Linux 桌面环境
- Node.js 20+
- Rust stable
- Google Chrome
- Tauri 2 所需系统依赖

如果只是开发前端和 Rust 逻辑，通常执行 `npm ci` 后就可以开始。

```bash
npm ci
```

## 本地开发

启动桌面应用：

```bash
npm run tauri:dev
```

或者：

```bash
make dev
```

启动后，在桌面界面里先添加平台账号。账号可用后，再通过本地 API 调用生成图片。

## 构建应用

```bash
npm run tauri:build
```

或者：

```bash
make build
```

构建产物会由 Tauri 输出到 `src-tauri/target/` 下对应平台目录。

## OpenAI 兼容 API

默认本地 API 地址是：

```text
http://127.0.0.1:18080
```

端口可以在桌面端设置页面调整。

### 查看模型

```bash
curl http://127.0.0.1:18080/v1/models
```

返回结构示例：

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

### 文字生成图片

```bash
curl http://127.0.0.1:18080/v1/images/generations \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "flow_Nano_Banana_Pro",
    "prompt": "A clean product photo of a glass water bottle on a white desk",
    "size": "1024x1024",
    "quality": "high",
    "output_format": "png"
  }'
```

返回结构示例：

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

### 根据参考图编辑

`images[].image_url` 支持公网图片 URL，也支持 `data:image/png;base64,...` 这种 data URL。

```bash
curl http://127.0.0.1:18080/v1/images/edits \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "flow_Nano_Banana_Pro",
    "prompt": "Turn this image into a clean studio product shot",
    "images": [
      {
        "image_url": "https://example.com/source.png"
      }
    ],
    "size": "1024x1024",
    "quality": "high",
    "output_format": "png"
  }'
```

### 在 OpenAI SDK 中使用

把 SDK 的 `baseURL` 指向 Frame64 本地服务即可。

```ts
import OpenAI from 'openai'

const client = new OpenAI({
  apiKey: 'frame64-local',
  baseURL: 'http://127.0.0.1:18080/v1',
})

const result = await client.images.generate({
  model: 'flow_Nano_Banana_Pro',
  prompt: 'A minimal desk setup with a silver laptop and soft daylight',
  size: '1024x1024',
})

const imageBase64 = result.data?.[0]?.b64_json
```

## 接口注意事项

- Frame64 只返回 `b64_json`，不返回远程图片 URL。
- 当前图片接口不支持 `stream: true`，请求会在生成完成后一次性返回 JSON。
- 当前不提供 OpenAI Files API，所以编辑图片请使用 `images[].image_url`。
- `n` 会被限制在 `1..4`，避免单次请求占用过多浏览器和平台资源。
- `size` 支持 `auto`、`1024x1024`、`1536x1024`、`1024x1536`。
- `/v1/images/variations` 当前会返回不支持，后续可按插件能力扩展。

## 插件机制

每个平台插件通常包含：

```text
plugins/{platform}/
├── manifest.json      # 平台、模型、账号配额、执行步骤
└── scripts/           # 页面检测、输入、轮询、下载等脚本
```

核心原则是“插件薄、核心稳”：

- 平台页面怎么点、接口怎么轮询，优先放在插件里。
- Rust `engine/` 只负责浏览器、HTTP、文件、账号等通用原子能力。
- `orchestrator.rs` 只读取 manifest 步骤并按顺序执行，不写平台专属流程。

## 常用命令

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

## 开发约定

- React 组件不要直接调用 Tauri command，统一走 service 层。
- `src/shared` 不能反向依赖 `src/features` 或 `src/app`。
- 单文件尽量保持在 500 行以内，复杂页面拆成组件、hooks、services。
- 新增核心 service、hook、API 或重要分支时，需要补测试。
- 不提交 `.env`、密钥、缓存目录、构建产物或平台登录数据。

## 文档

- [OpenAI 图片接口参考](docs/api/openai-api.md)
- [图片生成接口](docs/api/create-image.md)
- [图片编辑接口](docs/api/edits-image.md)
- [图片变体接口](docs/api/image-variation.md)

## License

当前仓库还没有声明开源许可证。正式公开前，请先补充 `LICENSE` 文件。
