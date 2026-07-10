### Multipart Fields

用于本地上传参考图。

字段建议：

| 字段          | 类型 | 说明        |
| ------------- | ---- | ----------- |
| `model`       | text | 模型 ID     |
| `prompt`      | text | 提示词      |
| `seconds`     | text | 时长        |
| `size`        | text | 尺寸        |
| `quality`     | text | 质量        |
| `image`       | file | 主参考图    |
| `image[]`     | file | 多张参考图  |
| `first_frame` | file | 首帧        |
| `last_frame`  | file | 末帧        |
| `metadata`    | text | JSON 字符串 |

### Response

创建成功返回：

```json
{
  "id": "video_abc123",
  "object": "video",
  "created_at": 1780000000,
  "status": "queued",
  "model": "sora-video",
  "progress": 0,
  "seconds": 8,
  "size": "1280x720",
  "expires_at": 1780003600
}
```

字段说明：

| 字段         | 说明                       |
| ------------ | -------------------------- |
| `id`         | 视频任务 ID                |
| `object`     | 固定为 `video`             |
| `created_at` | Unix 秒                    |
| `status`     | 任务状态                   |
| `model`      | 模型 ID                    |
| `progress`   | 进度，0 到 100             |
| `seconds`    | 目标时长                   |
| `size`       | 目标尺寸                   |
| `expires_at` | 本地结果保留到期时间，可选 |

### Status

视频任务状态建议统一为：

- `queued`
- `in_progress`
- `completed`
- `failed`
- `cancelled`

如果插件返回其他状态，服务端要映射成这几个标准状态。

### Example: 文生视频

```bash
curl http://127.0.0.1:18080/v1/videos \
  -H "Authorization: Bearer $MEDIAGEN_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "sora-video",
    "prompt": "A cinematic product video of a glass water bottle rotating on a clean white desk",
    "seconds": 8,
    "size": "1280x720",
    "quality": "high"
  }'
```

### Example: 参考图生成视频

```bash
curl http://127.0.0.1:18080/v1/videos \
  -H "Authorization: Bearer $MEDIAGEN_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "sora-video",
    "prompt": "Make this product slowly rotate with soft studio lighting",
    "seconds": 8,
    "size": "1280x720",
    "input_reference": {
      "type": "input_image",
      "image_url": "data:image/png;base64,..."
    }
  }'
```

### Example: multipart 上传参考图

```bash
curl http://127.0.0.1:18080/v1/videos \
  -H "Authorization: Bearer $MEDIAGEN_API_KEY" \
  -F "model=sora-video" \
  -F "prompt=Make this product slowly rotate with soft studio lighting" \
  -F "seconds=8" \
  -F "size=1280x720" \
  -F "image=@./product.png"
```

## 查询视频任务

### GET `/v1/videos/{video_id}`

查询视频任务状态。

### Response: in_progress

```json
{
  "id": "video_abc123",
  "object": "video",
  "created_at": 1780000000,
  "status": "in_progress",
  "model": "sora-video",
  "progress": 45,
  "seconds": 8,
  "size": "1280x720"
}
```

### Response: completed

```json
{
  "id": "video_abc123",
  "object": "video",
  "created_at": 1780000000,
  "status": "completed",
  "model": "sora-video",
  "progress": 100,
  "seconds": 8,
  "size": "1280x720",
  "content_url": "http://127.0.0.1:18080/v1/videos/video_abc123/content",
  "thumbnail_url": "http://127.0.0.1:18080/v1/videos/video_abc123/content?variant=thumbnail"
}
```

### Response: failed

```json
{
  "id": "video_abc123",
  "object": "video",
  "created_at": 1780000000,
  "status": "failed",
  "model": "sora-video",
  "progress": 0,
  "error": {
    "message": "插件执行失败",
    "code": "task_failed"
  }
}
```

## 下载视频内容

### GET `/v1/videos/{video_id}/content`

下载生成的视频文件。

默认返回主视频。

### Query Parameters

#### `variant: string`

可选。

建议支持：

- `video`
- `thumbnail`
- `spritesheet`

默认：

- `video`

### Response

视频内容返回：

```http
Content-Type: video/mp4
Content-Disposition: inline; filename="video_abc123.mp4"
```

缩略图返回：

```http
Content-Type: image/jpeg
```

### Example

```bash
curl http://127.0.0.1:18080/v1/videos/video_abc123/content \
  -H "Authorization: Bearer $MEDIAGEN_API_KEY" \
  --output result.mp4
```

## 删除视频任务

### DELETE `/v1/videos/{video_id}`

删除本地视频任务和结果文件。

### Response

```json
{
  "id": "video_abc123",
  "object": "video.deleted",
  "deleted": true
}
```

## 视频 SSE 事件

如果支持 `stream: true`，建议使用 SSE。

### 创建事件

```text
event: video.created
data: {"type":"video.created","id":"video_abc123","status":"queued"}
```

### 进度事件

```text
event: video.progress
data: {"type":"video.progress","id":"video_abc123","status":"in_progress","progress":45}
```

### 完成事件

```text
event: video.completed
data: {"type":"video.completed","id":"video_abc123","status":"completed","content_url":"/v1/videos/video_abc123/content"}
```

### 失败事件

```text
event: video.failed
data: {"type":"video.failed","id":"video_abc123","status":"failed","error":{"code":"task_failed","message":"插件执行失败"}}
```
