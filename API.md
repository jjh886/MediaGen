# MediaGen 本地 API 接口文档

本文档面向接入 MediaGen 桌面端本地服务的应用。接口整体按 OpenAI Image API 风格设计，接入方可以把 MediaGen 当作一个本机的 OpenAI 兼容图片服务使用。

## 基础地址

MediaGen 默认只监听本机地址：

```text
http://127.0.0.1:18080
```

为了绕开浏览器和很多 HTTP/1.1 客户端“同一个 origin 最多约 6 条并发连接”的限制，MediaGen 默认会同时监听 10 个连续端口：

```text
http://127.0.0.1:18080
http://127.0.0.1:18081
http://127.0.0.1:18082
http://127.0.0.1:18083
http://127.0.0.1:18084
http://127.0.0.1:18085
http://127.0.0.1:18086
http://127.0.0.1:18087
http://127.0.0.1:18088
http://127.0.0.1:18089
```

这 10 个端口提供完全相同的 API。高并发调用时，建议接入方按轮询方式分配请求，例如第 1 个请求走 `18080`，第 2 个走 `18081`，一直到 `18089` 后再回到 `18080`。这样每个 `host:port` 都是不同 origin，可以显著降低 HTTP/1.1 长连接、SSE 连接被 6 并发限制卡住的概率。

## 端口配置

默认配置如下：

| 配置项                | 默认值      | 说明                                                             |
| --------------------- | ----------- | ---------------------------------------------------------------- |
| `api_address`         | `127.0.0.1` | API 监听地址                                                     |
| `api_port`            | `18080`     | 起始端口                                                         |
| `api_port_count`      | `10`        | 从起始端口开始监听多少个端口，范围 `1..32`                       |
| `api_end_port`        | 空          | 如果设置，则按 `api_port..api_end_port` 生成端口列表，最多 32 个 |
| `api_ports`           | 空          | 如果设置成端口列表，会优先使用该列表                             |
| `api_allowed_origins` | `*`         | CORS 允许来源；即使收紧配置，本机 localhost 来源仍会放行         |

端口优先级是：`api_ports` > `api_end_port` > `api_port_count`。

## 鉴权说明

本地 API 当前不校验 `Authorization`，但为了兼容 OpenAI SDK 和第三方应用，可以照常传：

```http
Authorization: Bearer local
```

真正的平台账号、Cookie、额度和排队逻辑由 MediaGen 桌面端内部管理。

## GET /v1/models

返回当前可用模型列表，包含插件真实模型，也包含 OpenAI 兼容别名，例如：

```text
gpt-image-2
gpt-image-1.5
gpt-image-1
gpt-image-1-mini
chatgpt-image-latest
```

示例：

```bash
curl http://127.0.0.1:18080/v1/models
```

响应结构：

```json
{
  "object": "list",
  "data": [
    {
      "id": "gpt-image-1",
      "object": "model",
      "owned_by": "openai",
      "display_name": "gpt-image-1",
      "platform": "chatgpt",
      "alias": true
    }
  ]
}
```

## POST /v1/images/generations

根据文本提示词生成图片。普通响应返回 JSON；`stream=true` 时返回 SSE。

### JSON 请求示例

```bash
curl http://127.0.0.1:18080/v1/images/generations \
  -H 'Content-Type: application/json' \
  -H 'Authorization: Bearer local' \
  -d '{
    "model": "gpt-image-1",
    "prompt": "A clean product photo of a glass water bottle on a white desk",
    "size": "1024x1024",
    "quality": "high",
    "output_format": "png",
    "response_format": "b64_json",
    "n": 1
  }'
```

### 常用参数

| 参数                 | 类型    | 说明                                                     |
| -------------------- | ------- | -------------------------------------------------------- |
| `model`              | string  | 必填。使用 `/v1/models` 返回的模型 ID 或 OpenAI 兼容别名 |
| `prompt`             | string  | 必填。图片提示词                                         |
| `n`                  | number  | 图片数量，当前限制为 `1..4`                              |
| `size`               | string  | `auto`、`1024x1024`、`1536x1024`、`1024x1536`            |
| `quality`            | string  | `auto`、`low`、`medium`、`high`                          |
| `response_format`    | string  | 只支持 `b64_json`，也可以省略                            |
| `output_format`      | string  | `png`、`jpeg`、`webp`，默认 `png`                        |
| `output_compression` | number  | `0..100`，主要用于 `jpeg` / `webp`                       |
| `background`         | string  | `auto`、`transparent`、`opaque`                          |
| `stream`             | boolean | `true` 时使用 SSE                                        |
| `partial_images`     | number  | OpenAI 兼容参数，范围 `0..3`；当前只保证最终图事件       |
| `user`               | string  | OpenAI 兼容参数，本地服务仅接收不强依赖                  |

### JSON 响应示例

```json
{
  "created": 1779695788,
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

返回图片会按请求的 `size` 和 `output_format` 做归一化处理。也就是说，如果响应里写的是 `1024x1024`、`jpeg`，`b64_json` 解出来的图片也会是对应尺寸和格式。

## POST /v1/images/generations SSE

开启 `stream=true`：

```bash
curl -N http://127.0.0.1:18080/v1/images/generations \
  -H 'Content-Type: application/json' \
  -H 'Accept: text/event-stream' \
  -d '{
    "model": "gpt-image-1",
    "prompt": "A minimal blue cube on a white background",
    "size": "1024x1024",
    "quality": "low",
    "stream": true,
    "partial_images": 0
  }'
```

MediaGen 的最终完成事件保持 OpenAI 图片流风格，同时额外增加了几个本地增强事件，方便前端展示排队状态并保持连接不断开。

### 本地增强事件

```text
event: image_generation.started
data: {"type":"image_generation.started","request_id":"...","model":"gpt-image-1"}

event: image_generation.queued
data: {"type":"image_generation.queued","request_id":"...","queued":0,"ahead":0,"queue_position":1,"active":0,"max_concurrent":64}

event: image_generation.account_queued
data: {"type":"image_generation.account_queued","request_id":"...","platform":"chatgpt","reason":"busy","ahead":1,"queue_position":2}

event: heartbeat
data: {"type":"heartbeat"}
```

说明：

- `started` 表示 SSE 连接已经建立，请求已经进入本地流程。
- `queued` 表示进入全局图片任务队列。
- `account_queued` 表示平台账号暂时忙、冷却或并发占满，正在等可用账号。
- `heartbeat` 默认约 10 秒一次，用来避免前端、代理或客户端认为连接空闲而断开。

### 最终完成事件

```text
event: image_generation.completed
data: {"type":"image_generation.completed","b64_json":"...","created_at":1779695864,"size":"1024x1024","quality":"low","output_format":"png"}
```

接入方如果只想兼容 OpenAI 风格，可以忽略非 `image_generation.completed` / `error` 的事件。

## POST /v1/images/edits

根据参考图和提示词进行图片编辑。接口支持两种传图方式：

1. JSON：使用 `images[].image_url`，可传远程 URL 或 data URL。
2. multipart/form-data：兼容 OpenAI SDK 常见的文件上传字段。

### JSON 请求示例

```bash
curl http://127.0.0.1:18080/v1/images/edits \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "gpt-image-1",
    "prompt": "Keep the product, change the background to a clean kitchen counter",
    "images": [
      {
        "image_url": "data:image/png;base64,..."
      }
    ],
    "size": "1024x1024",
    "output_format": "png"
  }'
```

### multipart 请求示例

```bash
curl http://127.0.0.1:18080/v1/images/edits \
  -F model=gpt-image-1 \
  -F prompt='Change the background to a clean kitchen counter' \
  -F image='@/absolute/path/input.png' \
  -F size=1024x1024 \
  -F output_format=png
```

编辑接口也支持 `stream=true`，最终事件类型是：

```text
event: image_edit.completed
data: {"type":"image_edit.completed","b64_json":"...","created_at":1779695864,"size":"1024x1024","quality":"auto","output_format":"png"}
```

当前本地桌面端不提供 OpenAI Files API，所以 `file_id` 不可用。`mask` 参数目前也不支持，会返回 OpenAI 风格错误。

## POST /v1/images/variations

当前不支持，会返回错误。接入方不要依赖该接口。

## 错误响应

普通 JSON 请求失败时，返回 OpenAI 风格错误：

```json
{
  "error": {
    "message": "模型 not-a-real-model 不存在",
    "type": "invalid_request_error",
    "param": null,
    "code": "model_not_found"
  }
}
```

SSE 请求失败时，会发送：

```text
event: error
data: {"error":{"message":"模型 not-a-real-model 不存在","type":"invalid_request_error","param":null,"code":"model_not_found"},"request_id":"..."}
```

## 并发接入建议

图片生成通常是长耗时请求，尤其 SSE 会长时间占用连接。为了避免 HTTP/1.1 单 origin 6 并发限制，建议：

1. 启动时先请求任意端口的 `/v1/models`，确认服务可用。
2. 维护端口池 `18080..18089`。
3. 生成或编辑请求按轮询分发到不同端口。
4. SSE 客户端只把 `image_generation.completed`、`image_edit.completed` 和 `error` 当作最终状态，其它事件用于 UI 提示和保活。

简单端口选择示例：

```js
const ports = [18080, 18081, 18082, 18083, 18084, 18085, 18086, 18087, 18088, 18089]
let next = 0

function pickBaseUrl() {
  const port = ports[next % ports.length]
  next += 1
  return `http://127.0.0.1:${port}`
}
```
