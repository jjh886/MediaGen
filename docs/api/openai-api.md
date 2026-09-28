# OpenAI 兼容接口文档

图片生成文档按章节拆分：

- [01 Generate Images](openai-api/01-generate-images.md)
- [02 Streaming and Edit Images](openai-api/02-streaming-edit-images.md)
- [03 Output, Limitations and Cost](openai-api/03-output-limits-cost.md)

视频任务接口见 [OpenAI 兼容视频接口](video-api.md)。

图片模型 `ChatGPT` 使用控制台中的平台模型配置；未配置或选择自动时，采用该账号会话初始化返回的默认模型。`chatgpt-image-*` 则使用明确指定的模型，不会把账号不支持的型号静默替换成其他型号。图片编辑中的本地参考图会先完成上传与处理，再提交编辑请求。

## 聊天附件

Codex 的纯文本和代码附件须使用 UTF-8 编码。插件将文件名和完整文本一起交给模型，避免上游丢失附件身份；PDF、Office 文档和图片仍通过原文件协议处理。

ChatGPT 聊天支持通过 `input_audio`（WAV / MP3）或 `input_file`（音频文件）提交语音。插件先调用同一账号的语音转写接口，再将带文件名和转写说明的文字交给聊天模型；这不等同于音色、音乐或环境声音分析。转写失败或没有有效语音文本时返回错误，不会把无法读取的普通文件上传结果当作成功。其他图片和文档附件继续保留在同一消息中。

## 自动获取会话名称

ChatGPT、豆包和 DeepSeek 文本插件在正文生成完成后读取平台已有的会话标题，通过 `metadata` 返回，不混入正文，也不会为生成标题额外调用模型。Codex 当前不提供此平台标题读取能力。

| 字段                          | 含义                                                   |
| ----------------------------- | ------------------------------------------------------ |
| `metadata.conversation_id`    | 上游会话 ID，仅获得有效标题时随标题返回                |
| `metadata.conversation_title` | 平台会话名称，合并空白并限制为最多 120 个 Unicode 字符 |

### 请求

```sh
curl "$BASE_URL/v1/responses" \
  -H "Authorization: Bearer $MEDIAGEN_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "doubao-text",
    "input": "帮我设计橙色杯子的广告文案",
    "store": true,
    "metadata": {"project": "cup"}
  }'
```

无需启用额外参数。账号池须授权给当前 API Key；模型须可用，可先通过 `/v1/models` 查询。

响应中的元数据示例（其余响应字段保持原协议）：

```json
{
  "metadata": {
    "project": "cup",
    "conversation_id": "upstream-conversation-id",
    "conversation_title": "橙色杯子广告"
  }
}
```

请求中自带的 `metadata` 与插件会话信息合并；同名字段以插件返回值为准。

### 普通响应与流式读取

| 接口                               | 标题位置                                                                        |
| ---------------------------------- | ------------------------------------------------------------------------------- |
| `POST /v1/responses` 非流式        | 响应顶层 `metadata.conversation_title`                                          |
| `POST /v1/responses` SSE           | `response.completed` 事件的 `response.metadata.conversation_title`              |
| `WS /v1/responses`                 | `response.completed` 消息的 `response.metadata.conversation_title`              |
| `POST /v1/chat/completions` 非流式 | 响应顶层 `metadata.conversation_title`（MediaGen 扩展）                         |
| `POST /v1/chat/completions` SSE    | `finish_reason` 非空的结束帧顶层 `metadata.conversation_title`（MediaGen 扩展） |
| `GET /v1/responses/{response_id}`  | 已保存响应顶层 `metadata.conversation_title`                                    |

Chat SSE 示例：

```text
data: {"id":"chatcmpl_example","object":"chat.completion.chunk","created":1790000000,"model":"doubao-text","choices":[{"index":0,"delta":{},"finish_reason":"stop","logprobs":null}],"metadata":{"conversation_id":"upstream-conversation-id","conversation_title":"橙色杯子广告"}}

data: [DONE]
```

客户端应独立读取标题，不要拼入 `choices[].delta.content`。有 `include_usage` 时，结束帧之后还可能有用量帧，再出现 `[DONE]`。

标题读取最多尝试 3 次，每次网络请求超时 3 秒，遇到默认或空标题时等待 500 毫秒再试；异常直接停止标题读取。标题无效或读取失败不会让正文任务失败，而是省略插件标题字段。`store=true` 的 Responses 会保留标题，查询须使用同一 API Key；`store=false` 无法事后查询。

`conversation_id` 属于平台会话信息，不能替代 MediaGen 的 `resp_*` 响应 ID；续接仍使用 `previous_response_id`。当前没有独立的重命名或查询平台标题接口。
