export interface ApiExample {
  group: string
  method: 'GET' | 'POST' | 'DELETE' | 'WS'
  path: string
  title: string
  description: string
  notes: string[]
  body?: Record<string, unknown>
  form?: string[]
  response: unknown
  status?: number
}
export const response = {
  id: 'resp_example',
  object: 'response',
  status: 'completed',
  model: 'gpt-5.6-luna',
  output: [
    {
      type: 'message',
      role: 'assistant',
      content: [{ type: 'output_text', text: '你好', annotations: [] }],
    },
  ],
  usage: { input_tokens: 8, output_tokens: 2, total_tokens: 10 },
}
export const image = {
  created: 1790000000,
  data: [{ b64_json: '<图片的完整 Base64 编码>' }],
  output_format: 'png',
}
export const file = {
  id: 'file_example',
  object: 'file',
  bytes: 1024,
  created_at: 1790000000,
  filename: 'reference.png',
  purpose: 'vision',
}
export const video = {
  id: 'video_example',
  object: 'video',
  model: 'grok-imagine-video-1.5-preview',
  status: 'queued',
  progress: 0,
  seconds: '6',
  size: '1280x720',
}
export const textNotes = [
  'model（必填）：/v1/models 返回的文本模型 ID。',
  'input（Responses）或 messages（Chat，必填）：文本、消息数组、图片/文件内容及工具调用历史。',
  'stream：默认 false；true 返回 SSE。Chat 可用 stream_options.include_usage=true 获取最后一帧用量。',
  'instructions：系统指令；tools / tool_choice / parallel_tool_calls：工具与选择方式，依模型能力而定。',
  'reasoning.effort（Responses）/ reasoning_effort（Chat）：以模型 capabilities.reasoning.efforts 为准。',
  'text.format（Responses）/ response_format（Chat）：text、json_object 或 json_schema 结构化输出。',
  'max_output_tokens（Responses）/ max_completion_tokens（Chat）、temperature、top_p 等参数须查询模型能力；不支持时返回 400，不保证每个平台支持相同字段。',
  'prompt_cache_key（模型支持时）：同一对话保持不变，优先沿用同一账号；不支持该参数时可使用 previous_response_id 续接，或保留完整消息历史，按首条用户消息尽力保持绑定。不同 API Key 的绑定隔离。',
  'previous_response_id（Responses）：续接同一模型、同一 API Key 保存的响应；store 默认 true，false 不保存；WebSocket 默认不保存，仅当前连接可续接。',
  'metadata（Responses）：最多 16 个字符串键值；background=true 暂不支持。所有可用输入以模型 capabilities.input_schema 为准。',
]
export const imageNotes = [
  'model、prompt（必填）；n：1–4；size：auto 或 宽x高；具体尺寸与能力可查模型目录。',
  'quality、background、moderation、input_fidelity：取值按模型 capabilities.limits.parameter_values。',
  'response_format：仅 b64_json；output_format：png/jpeg/webp；output_compression：0–100，仅 jpeg/webp。',
  'stream=true 返回 image_generation.completed / image_edit.completed 等 SSE；partial_images：0–3，仅支持预览的模型有效。',
  'async=true 或 sync=false：仅图片生成进入持久化队列并返回 202；省略时同步返回。排队与 stream=true 不可同时使用。',
  'callback_url（别名 webhook_url）：仅排队生成可用；完成后 POST 图片 Base64 和任务状态，回调失败有重试。',
  '编辑时 image/image[] 文件使用 multipart/form-data；也可 JSON images 数组（image_url 为 Data URL 或 file_id）；mask 仅支持遮罩的模型接受。',
  'user：可选用户标识；image_aspect_ratio：比例扩展。图片内容从不以外部下载 URL 返回。',
]
