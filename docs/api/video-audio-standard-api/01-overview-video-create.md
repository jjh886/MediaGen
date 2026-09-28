# 视频、音频生成与音频转文字标准 API

> 本文保留早期设计方案，不代表全部接口已实现。当前豆包模型、实际请求字段和查询下载流程以 [视频接口文档](../video-api.md) 及控制台「接口文档」为准。

## 设计目标

MediaGen 已经具备“插件执行任务、自动生成图片、暴露 OpenAI 兼容图片 API”的能力。
后续视频和音频也应该沿用同一套思想：

- 对外尽量贴近主流 OpenAI 兼容接口。
- 对内仍然通过插件声明能力和执行流程。
- 标准字段优先，平台特殊字段放到 `extra`。
- 任务结果统一落到 MediaGen 的本地结果结构里。
- 不把某个具体平台的参数硬编码成服务端通用逻辑。

视频生成、音频生成、音频转文字的接口要解决三类需求：

- 文本生成视频。
- 参考图生成视频。
- 文本生成语音。
- 音频转文字。
- 后续扩展音频翻译、音频编辑、配音、音乐生成等能力。

## 标准来源

本方案优先参考 OpenAI 当前公开接口形状：

- Video API：`POST /v1/videos`、`GET /v1/videos/{video_id}`、`GET /v1/videos/{video_id}/content`。
- Audio Speech API：`POST /v1/audio/speech`。
- Audio Transcriptions API：`POST /v1/audio/transcriptions`。

注意：视频模型名不要写死在 MediaGen 服务端。

截至 2026-07-02，OpenAI 官方文档中的 Sora 2 视频模型已有退役提示，官方接口形状仍然值得兼容，但具体模型应该由插件 manifest 和 `/v1/models` 返回。

## 总体接口规划

首期建议实现以下接口：

| 能力         | 方法   | 路径                            | 类型             |
| ------------ | ------ | ------------------------------- | ---------------- |
| 视频生成     | POST   | `/v1/videos`                    | 异步任务         |
| 查询视频任务 | GET    | `/v1/videos/{video_id}`         | 任务状态         |
| 下载视频内容 | GET    | `/v1/videos/{video_id}/content` | 二进制           |
| 删除视频任务 | DELETE | `/v1/videos/{video_id}`         | 管理             |
| 音频生成     | POST   | `/v1/audio/speech`              | 同步或流式二进制 |
| 音频转文字   | POST   | `/v1/audio/transcriptions`      | 同步文本/JSON    |

后续可选接口：

| 能力         | 方法 | 路径                           | 说明              |
| ------------ | ---- | ------------------------------ | ----------------- |
| 视频列表     | GET  | `/v1/videos`                   | 方便管理任务      |
| 视频取消     | POST | `/v1/videos/{video_id}/cancel` | 长任务取消        |
| 视频编辑     | POST | `/v1/videos/edits`             | 局部修改视频      |
| 视频延长     | POST | `/v1/videos/extensions`        | 延长现有视频      |
| 音频翻译     | POST | `/v1/audio/translations`       | 兼容 Whisper 翻译 |
| 音频转写任务 | POST | `/v1/audio/transcription_jobs` | 长音频异步转写    |

## 通用认证

所有接口都使用和图片接口一致的认证方式：

```http
Authorization: Bearer <MediaGen API Key>
```

请求进入服务端后：

1. 校验 API Key 是否存在。
2. 校验 API Key 是否启用。
3. 校验 API Key 是否有账号池授权。
4. 根据模型找到对应插件和平台。
5. 根据 API Key 授权范围选择可用账号。
6. 执行插件任务。
7. 返回标准响应。

## 通用错误格式

所有接口统一返回 OpenAI 风格错误结构：

```json
{
  "error": {
    "message": "授权账号池中没有可用账号",
    "type": "invalid_request_error",
    "param": null,
    "code": "no_available_account"
  }
}
```

推荐错误码：

| code                        | HTTP | 说明                           |
| --------------------------- | ---- | ------------------------------ |
| `missing_api_key`           | 401  | 未传 Bearer Key                |
| `invalid_api_key`           | 401  | Key 不存在                     |
| `api_key_disabled`          | 403  | Key 已停用                     |
| `model_not_found`           | 404  | 模型不存在                     |
| `model_capability_mismatch` | 400  | 模型不支持当前任务类型         |
| `unsupported_parameter`     | 400  | 当前插件不支持某个参数         |
| `invalid_reference`         | 400  | 参考图、参考音频或文件 ID 无效 |
| `no_available_account`      | 503  | 授权账号池中没有可用账号       |
| `task_failed`               | 500  | 插件执行失败                   |
| `task_timeout`              | 504  | 插件执行超时                   |

## 模型能力声明

`/v1/models` 返回的视频和音频模型需要带上 `task_kind`。

示例：

```json
{
  "object": "list",
  "data": [
    {
      "id": "sora-video",
      "object": "model",
      "owned_by": "chatgpt",
      "task_kind": "video"
    },
    {
      "id": "tts-1",
      "object": "model",
      "owned_by": "openai-compatible",
      "task_kind": "audio_speech"
    },
    {
      "id": "whisper-1",
      "object": "model",
      "owned_by": "openai-compatible",
      "task_kind": "audio_transcription"
    }
  ]
}
```

推荐 `task_kind`：

- `image`：图片生成或编辑。
- `video`：视频生成。
- `audio_speech`：文本生成语音。
- `audio_transcription`：音频转文字。
- `audio_translation`：音频翻译。
- `audio_music`：音乐生成，后续扩展。

## 视频生成

### POST `/v1/videos`

创建视频生成任务。

视频生成通常耗时较长，因此接口应该按异步任务设计：

- 请求成功后立即返回 `video` 对象。
- `status` 初始为 `queued` 或 `in_progress`。
- 客户端轮询 `GET /v1/videos/{video_id}`。
- 完成后通过 `/content` 下载视频。

### Content-Type

支持两种请求格式：

- `application/json`：用于纯文本生成视频、远程 URL 参考图、文件 ID 参考图。
- `multipart/form-data`：用于上传本地参考图、首帧图、末帧图。

首期必须支持 `application/json`。

如果要支持参考图本地上传，必须支持 `multipart/form-data`。

### JSON Body Parameters

#### `model: string`

必填或可由服务端默认。

建议 MediaGen 首期要求必填，这样可以明确路由到哪个插件。

示例：

```json
{
  "model": "sora-video"
}
```

#### `prompt: string`

必填。

视频提示词。

示例：

```json
{
  "prompt": "A cinematic product video of a glass water bottle rotating on a clean white desk"
}
```

#### `seconds: number | string`

可选。

视频时长，单位秒。

建议兼容：

- `4`
- `8`
- `12`
- `"4"`
- `"8"`
- `"12"`

不同插件支持的时长不同。

如果插件不支持精确时长，服务端应该映射到最接近的平台选项。

#### `size: string`

可选。

视频尺寸。

推荐兼容 OpenAI 视频接口常见写法：

- `720x1280`
- `1280x720`
- `1024x1792`
- `1792x1024`

为了兼容更多国内外视频平台，也建议接受：

- `1080x1920`
- `1920x1080`
- `1024x1024`

如果用户传了 `aspect_ratio`，服务端可以映射成最接近的 `size`。

#### `aspect_ratio: string`

MediaGen 扩展字段。

可选。

用于兼容很多视频平台常见的比例参数。

建议支持：

- `16:9`
- `9:16`
- `1:1`
- `4:3`
- `3:4`
- `21:9`

当同时传 `size` 和 `aspect_ratio` 时：

- 优先使用 `size`。
- `aspect_ratio` 只作为插件参数补充。

#### `quality: string`

可选。

视频质量偏好。

建议支持：

- `low`
- `medium`
- `high`
- `auto`

如果插件不支持质量选择，可以忽略或映射到默认值。

#### `input_reference: object`

可选。

主参考输入。

用于“参考图生成视频”或“首帧生成视频”。

推荐结构：

```json
{
  "input_reference": {
    "type": "input_image",
    "image_url": "https://example.com/reference.png"
  }
}
```

也可以使用文件 ID：

```json
{
  "input_reference": {
    "type": "input_image",
    "file_id": "file-xxx"
  }
}
```

也可以使用 base64 data URL：

```json
{
  "input_reference": {
    "type": "input_image",
    "image_url": "data:image/png;base64,..."
  }
}
```

推荐语义：

- 没有 `input_reference`：文生视频。
- 有 `input_reference`：图生视频。
- 插件如果支持首帧控制，把它当首帧。
- 插件如果只支持风格参考，把它当参考图。

#### `reference_images: array`

MediaGen 扩展字段。

可选。

用于多个参考图，例如角色、风格、构图、产品图。

示例：

```json
{
  "reference_images": [
    {
      "type": "input_image",
      "image_url": "https://example.com/product.png",
      "role": "subject"
    },
    {
      "type": "input_image",
      "image_url": "https://example.com/style.png",
      "role": "style"
    }
  ]
}
```

`role` 可选值：

- `subject`：主体参考。
- `character`：人物或角色一致性参考。
- `style`：风格参考。
- `composition`：构图参考。
- `first_frame`：首帧。
- `last_frame`：末帧。
- `auto`：由插件判断。

如果平台不支持多参考图，服务端只传第一张，或在 `unsupported_parameter` 中明确提示。

#### `image: string | array`

兼容字段。

很多客户端会沿用图片编辑接口习惯传 `image`。

MediaGen 可以兼容：

```json
{
  "image": "data:image/png;base64,..."
}
```

或：

```json
{
  "image": ["data:image/png;base64,...", "https://example.com/style.png"]
}
```

服务端应转换为：

- 第一张进入 `input_reference`。
- 其他进入 `reference_images`。

#### `negative_prompt: string`

MediaGen 扩展字段。

可选。

负向提示词。

不是所有平台支持。

如果插件不支持，可以忽略，也可以按插件 manifest 的策略报错。

#### `seed: number`

MediaGen 扩展字段。

可选。

随机种子。

不是所有平台支持。

#### `background: string`

可选。

用于视频主体背景偏好。

建议只作为语义字段传给插件，不在服务端硬编码。

#### `camera_motion: string`

MediaGen 扩展字段。

可选。

常见值：

- `auto`
- `static`
- `pan_left`
- `pan_right`
- `zoom_in`
- `zoom_out`
- `orbit`
- `handheld`

如果插件不支持，忽略或报错。

#### `motion_strength: number`

MediaGen 扩展字段。

可选。

运动强度，建议范围 `0..1`。

#### `response_format: string`

可选。

建议支持：

- `url`
- `b64_json`

但视频文件可能很大，首期推荐只支持：

- `url`

这里的 URL 可以是 MediaGen 本地下载地址，不一定是公网地址。

#### `stream: boolean`

可选。

视频任务首期不建议做真正流式视频返回。

如果传 `true`，可以返回 SSE 任务事件：

- `video.created`
- `video.in_progress`
- `video.progress`
- `video.completed`
- `video.failed`
