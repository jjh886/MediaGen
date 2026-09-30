# Create Image

**post** `/v1/images/generations`

MediaGen 的图片生成接口按 OpenAI Image API 的响应结构返回结果。生成完成后不会上传到任何业务系统，图片字节会直接放在 `data[].b64_json` 中。

## Body Parameters

- `model: string`

  必填。使用 `/v1/models` 返回的模型 ID 或插件声明的别名。Codex 支持
  `gpt-image-2.5-flare`、`gpt-image-2.5-sunburst`、`gpt-image-2`，
  `codex` 别名指向 `gpt-image-2.5-flare`；其他平台的别名以模型目录为准。

- `prompt: string`

  必填。图片生成提示词。

- `n: optional number`

  生成图片数量。当前本地插件会把数量限制在 `1..4`。

- `size: optional "auto" or "1024x1024" or "1536x1024" or "1024x1536"`

  输出尺寸。MediaGen 会把它映射成平台侧比例选项。

- `quality: optional "low" or "medium" or "high" or "auto"`

  输出质量偏好。

- `output_format: optional "png" or "jpeg" or "webp"`

  返回图片格式偏好。默认 `png`。

- `response_format: optional "b64_json"`

  MediaGen 只返回 base64，不返回远程 URL。

- `stream: optional boolean`

  为 `true` 时返回 `text/event-stream`。当前插件会在生成完成后发送
  `image_generation.completed` 事件。

- `partial_images: optional number`

  OpenAI 兼容参数，范围 `0..3`。当前本地插件不生成中间预览图，所以只会收到最终事件。

## Returns

```json
{
  "created": 0,
  "data": [
    {
      "b64_json": "..."
    }
  ],
  "output_format": "png",
  "quality": "auto",
  "size": "1024x1024"
}
```

## Streaming Returns

```text
event: image_generation.completed
data: {"type":"image_generation.completed","b64_json":"...","created_at":0,"size":"1024x1024","quality":"high","output_format":"png"}
```

## Example

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
