# OpenAI 兼容视频接口

MediaGen 已实现异步视频任务接口。调用方提交后立即获得 `video_*` 任务 ID，再查询状态并下载成品；平台协议仍由插件负责，Rust 不包含豆包或 Doubao Work 的接口常量。

## 创建视频

```http
POST /v1/videos
Authorization: Bearer <MediaGen API Key>
Content-Type: multipart/form-data
```

标准字段：

- `model`：视频模型 ID，必填。
- `prompt`：原始视频提示词，必填。
- `input_reference`：可选参考图片文件、`file_id` 或 `image_url`。
- `seconds`：视频秒数，可传数字或数字字符串。
- `size`：`宽x高`，例如 `1280x720`。

JSON 也可使用：

```json
{
  "model": "doubaowork-seedance-2.5",
  "prompt": "一只纸折小船在清澈溪流中缓慢前行",
  "seconds": "5",
  "size": "1280x720"
}
```

创建响应：

```json
{
  "id": "video_...",
  "object": "video",
  "model": "doubaowork-seedance-2.5",
  "status": "queued",
  "progress": 0,
  "created_at": 1787810000,
  "seconds": "5",
  "size": "1280x720",
  "quality": "standard"
}
```

## 查询、列表与下载

```text
GET    /v1/videos
GET    /v1/videos/{video_id}
GET    /v1/videos/{video_id}/content
POST   /v1/videos/{video_id}/cancel
DELETE /v1/videos/{video_id}
```

`GET /v1/videos` 支持 `after`、`limit` 和 `order=asc|desc`。任务状态为 `queued`、`in_progress`、`completed` 或 `failed`。

下载接口的 `variant` 默认为 `video`；插件返回封面时也支持 `variant=thumbnail`。视频字节由服务端流式转发，不写入任务 JSON。

任务按调用 API Key 隔离。另一个 API Key 无法查询、下载、取消或删除不属于自己的视频。

## 当前插件模型

- `doubao-video`：复刻 `reference/doubao2api` 的 `content_type=2020/2021` 与 `skill_type=17` 视频链路。
- `doubaowork-seedance-2.5`：通过本机 Doubao Work App 的 CDP 和原生页面提交，固定使用 `Seedance 2.5`。
- `doubaowork-seedream-5.0-pro`：通过同一 App 的图片技能提交，固定使用 `Seedream 5.0 Pro`。

Doubao Work 插件不会接受模型降级。账号缺少 Seedance 2.5 或 Seedream 5.0 Pro 权益时，任务会明确失败，不会把 Seedance 2.0 Fast 或 Seedream 4.5 当作成功结果。

## Doubao Work 本机要求

插件会连接 `http://[::1]:9222`。若 Doubao Work 尚未运行，插件会尝试执行官方安装包：

```text
/Applications/DoubaoWork.app/Contents/MacOS/DoubaoWork
  --remote-debugging-port=9222
  --remote-allow-origins=*
```

如果 App 已经以普通模式运行，Chromium 不会在运行中补开 CDP。请先正常退出 Doubao Work，再重试 API 请求，让插件以调试模式启动它。

首次使用工作任务或新工具时，Doubao Work 可能显示权限选择。权限必须由用户本人在 App 中确认；插件只等待结果，不会代替用户扩大权限。

官方接口参考：<https://developers.openai.com/api/reference/typescript/resources/videos/methods/create>
