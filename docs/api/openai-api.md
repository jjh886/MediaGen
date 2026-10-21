# OpenAI 兼容接口文档

图片生成文档按章节拆分：

- [01 Generate Images](openai-api/01-generate-images.md)
- [02 Streaming and Edit Images](openai-api/02-streaming-edit-images.md)
- [03 Output, Limitations and Cost](openai-api/03-output-limits-cost.md)

视频任务接口见 [OpenAI 兼容视频接口](video-api.md)。

图片模型 `ChatGPT` 使用控制台中的平台模型配置；未配置或选择自动时，采用该账号会话初始化返回的默认模型。`chatgpt-image-*` 则使用明确指定的模型，不会把账号不支持的型号静默替换成其他型号。图片编辑中的本地参考图会先完成上传与处理，再提交编辑请求。

## 聊天附件

豆包图片识别回复同时支持内容区块与旧版文本协议。流式增量和完整快照会去重；若上游改写已经输出的正文，返回 `generation_incomplete` 并保留原会话供恢复。恢复时也可读取旧版文本正文。平台额度或订阅限制作为失败报告，不会把限制提示混入成功回复。

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

## 插件协议与模型同步

Codex 文本模型包括 `gpt-6.1-sol`、`gpt-6-astra`、`gpt-6-sol`、`gpt-6-luna`、`gpt-5.6-sol`、`gpt-5.6-terra` 和 `gpt-5.6-luna`。`gpt-6.1-sol` 的 `reasoning.effort` 支持 `low/medium/high/xhigh/max`，默认 `low`；其他型号优先按账号模型目录校验。目录不再列出插件不支持的上游型号。

Codex 图片模型为 `gpt-image-2.5-flare`、`gpt-image-2.5-sunburst`、`gpt-image-2`，`codex` 别名指向 `gpt-image-2.5-flare`。此前的 `gpt-image-1.5` 和日期后缀型号不再由 Codex 插件提供。模型 `capabilities.pricing` 同步源插件的参考价格，不表示当前账号的实际收费。

Codex 文本请求的 `service_tier` 支持 `auto/default/priority/flex/scale/ultrafast`，兼容 `fast` 并转成 `priority`；其他值返回参数错误，账号是否具有对应服务等级仍由上游判定。

`POST /v1/audio/speech` 的 `chatgpt-speech` 使用文本会话，确认回复与输入逐字相同后生成音频；不匹配时失败，避免朗读模型自行增加的内容。只支持 `speed=1`，不支持 `instructions`。MP3、Opus、AAC 使用平台原生输出，WAV、PCM、FLAC 真实转码后返回对应格式的二进制。

豆包在文本、图片和视频任务中自动执行账号续期与流式空闲保活。旧凭据若缺少续期协议会返回 `LOGIN_EXPIRED`，需要在账号页面重新登录以采集；仅完成验证码不一定包含这些新凭据。DeepSeek 会同步设备、客户端配置和辅助校验令牌，并按平台配置处理超时及保活。人工验证和登录失效继续走现有账号状态与微信通知流程，通知须已绑定并开启。

## 文本聊天返回图片、视频和思考摘要

`POST /v1/responses` 与 `POST /v1/chat/completions` 可在普通聊天中交付平台生成的媒体。ChatGPT 插件支持聊天图片，豆包支持聊天图片和视频；具体模型和账号能否生成仍由平台判定。DeepSeek 的思考摘要也会单独返回。输出能力可查询 `/v1/models` 的 `capabilities.output_modalities`。

Responses 的 `output` 保留以下输出项；`output_text` 只包含回答正文：

| type                    | 结果与状态                                                                     |
| ----------------------- | ------------------------------------------------------------------------------ |
| `image_generation_call` | `result` 为纯 Base64，`output_format` 为 png/jpeg/webp；完成前 `result` 可为空 |
| `video_generation_call` | `result_url` 为平台 HTTPS 视频地址，`output_format` 为 mp4                     |
| `reasoning`             | `summary` 为 `summary_text` 数组，与正文分开                                   |

媒体项的 `id` 在本次响应中保持稳定，`status` 反映 `in_progress`、`generating`、`completed` 等状态。图片可能附带 width/height，视频可能附带 duration/provider_media_id。

SSE 与 Responses WebSocket 使用 `response.output_item.added` / `response.output_item.done` 返回媒体和摘要快照，摘要增量使用 `response.reasoning_summary_text.delta`。客户端按输出项 `id` 更新内容；收到 `response.completed` 后以最终 `response.output` 为准。图片 Base64 不应当作文字增量拼接。

Chat Completions 使用以下 **MediaGen 扩展字段**，普通 `message.content` / `delta.content` 仍表示回答正文：

- 非流式：`choices[0].message.media` 为图片、视频输出项数组；`message.reasoning` 为完整摘要项，`message.reasoning_content` 为摘要文本。
- 流式：`choices[0].delta.media` 与 `delta.reasoning` 按 `id` 更新快照；`delta.reasoning_content` 为可追加的摘要增量，最终摘要以 reasoning.summary 为准。
- 只有图片或视频时 `message.content` 可以为 null；不能以是否有文字判断生成是否成功。

`store=true` 保存完整 Responses 媒体输出，读取仍受原 API Key 权限限制。`previous_response_id` 和同连接 WebSocket 续接会保留生成图片作为图片输入、生成视频作为地址文本引用；不会因新增生成项而把整轮历史判为非法。Chat 客户端回传助手 `media` 时使用相同规则。平台视频地址可能过期，保存响应不会延长地址有效期。

豆包新登录流程会校验账号续期协议是否采集完整。缺少这些协议时报告 `AUTH_PROTOCOL_INCOMPLETE`；需要在账号页面重新登录完成采集。

## 音频附件与上下文更新

ChatGPT 聊天的音频附件通过文件表单上传到转写接口，保留文件名和 MIME 类型。同一请求中，历史消息和本轮引用完全相同的音频字节时只转写一次；同名但内容不同的音频分别处理。TTS 同时识别完整消息与后续增量，只有最终文本逐字符合输入时才生成音频。

Codex 将 `system` 消息转换成 `developer` 消息一次，显式 `instructions` 保留。上游明确拒绝超长上下文（包括 instructions 字符超限）时，返回带 `context_length_exceeded` 的参数错误并重置提交标记，调用方可压缩上下文后重试。

网页文本插件不再把 `max_output_tokens` 改写成提示词中的字数要求。该参数是否有硬性限制效果取决于模型的原生能力。

ChatGPT 的 `options.fixed_video_analysis: true` 是可选扩展，用于固定视频分析。首次提交和任务恢复共用 20 分钟等待预算；恢复时优先读取原会话已有结果。仅本机查询排队继续等待，真实上游 429 按 `Retry-After` 转为重试并保留已上传附件。默认关闭；已有会话续接仍遵守原会话边界，不会静默换成其他会话。
