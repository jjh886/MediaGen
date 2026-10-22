# Create Image

**post** `/v1/images/generations`

MediaGen 的图片生成接口按 OpenAI Image API 的响应结构返回结果。生成完成后不会上传到任何业务系统，图片字节会直接放在 `data[].b64_json` 中。

## 长任务与恢复

长图片任务优先使用 `async: true` 获取任务 ID，再通过 `/api/v2/generation-tasks/{taskId}` 查询原任务；收到任务 ID 后不要再次提交生成。同步流式请求会发送保活注释，但调用方仍需设置适合长任务的超时。

同步生成、编辑和变体请求可携带 `X-Client-Request-Id`。恢复时必须保持同一 API Key、接口、请求编号和请求参数，服务端会串行恢复原任务、原账号和插件检查点；已完成结果加密保存后直接回放，不重复生成或累计本地账号次数。主动取消的任务不会自动恢复。一次新的生成须使用新编号。该编号不会让不同请求参数合并，也不会为 `async: true` 重复提交提供去重保证。

上游已受理但暂未完成时可能返回 `503 temporary_unavailable`，应沿用原编号恢复，不能因 HTTP 超时或暂时失败改用新编号。若缺少上游任务编号且提交结果不明，插件会报错保留状态，不冒险重投。

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

- `reasoning_effort: optional string`

  MediaGen 扩展思考深度参数，支持 JSON 和 multipart 图片请求。仅接受目标模型 `capabilities.reasoning.efforts` 声明的值，原样传给插件；未声明支持或指定了无效档位时返回 `400 unsupported_parameter`，不会自动降档或换模型。例如 `chatgpt-image-gpt-5-6` 支持 `none / min / standard / extended / max`，`chatgpt-image-gpt-6-pro` 当前仅声明 `standard`。Codex 图片模型不支持该参数，即使传 `none` 也会拒绝。省略时沿用模型或平台现有默认行为。

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
