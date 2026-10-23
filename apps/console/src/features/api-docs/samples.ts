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
  model: 'gpt-6-luna',
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
  'HTTP 文本接口默认同步；async=true 返回 HTTP 202 和 data.taskId，可传 callback_url 接收结果，或使用 /api/v2/generation-tasks/{task_id} 查询结果。不能同时 stream=true；此扩展不适用于 WebSocket、compact 或音频接口。',
  '图片和文本队列独立运行。文本并发在账号配置调整，默认值由插件提供（仅 ChatGPT 为 3，其余为 0，表示不设本地文本并发上限），填 0 使用插件默认值；临时失败由队列等待重试，取消和插队复用 generation-queue/actions。',
  '聊天可返回媒体：ChatGPT 支持图片，豆包支持图片和视频，实际能力取决于账号与平台。Responses 保留 image_generation_call / video_generation_call；Chat 使用 MediaGen 扩展 message.media，SSE 使用 delta.media。',
  '媒体进度与完成项按 id 更新；图片 result 为 Base64，视频 result_url 为有时效的 HTTPS 地址。思考摘要独立于正文；Responses 读取 reasoning.summary，Chat 读取 reasoning 或 reasoning_content。',

  'ChatGPT、豆包、DeepSeek 完成后可返回 metadata.conversation_title / conversation_id；Responses 在最终 response 中读取，Chat 流式在结束帧读取。标题获取失败时省略，不影响正文；Codex 不提供此平台标题读取能力。',
  'ChatGPT、Codex 的文档与聊天视频走 input_file（Responses）或 file（Chat）内容块，先上传 /v1/files 后引用 file_id；生成视频使用 /v1/videos。',
  '多媒体文件单个不超过 50 MiB、去重后累计不超过 64 MiB；视频最长 1 小时。不支持原生视频或文件超过 16 MiB 时最多抽取 8 帧，音轨按模型能力原生传入或转写。转写需要当前 Key 有可用的转写模型授权。',
  '豆包聊天支持图片、文档、音频，视频附件会经网关转换后再校验；豆包视频生成的参考视频仅在相应 Seedance 模型支持时可用。DeepSeek 不支持的输入类型会拒绝，具体以 /v1/models 为准。',
  '豆包图片识别回复兼容旧文本协议；流式增量与完成快照去重，恢复时从原会话读取正文。额度耗尽和订阅限制作为失败返回，不混入正文。',
  '字段表描述当前网关的协议。标为“由模型决定”或“模型支持时”的参数，需要先查询 /v1/models；不支持的参数返回 400。',
  'HTTP Responses 默认保存响应；Chat 不支持 store=true；WebSocket 的本地续接默认仅在当前连接内有效。',
  '图片、文件输入和原生工具调用按模型能力开放；示例展示请求结构，不代表所有模型均支持全部字段。',
]
export const imageNotes = [
  '具体尺寸、质量、参考图数量等限制以模型能力为准。模型未声明的能力不会自动实现。',
  '图片一律返回 Base64；图片排队仅支持 generations，且不能与 stream=true 同时使用。文本可用 async=true 进入独立队列。',
  '参数表中的默认模型可能未安装或未授权，建议始终显式填写 /v1/models 返回的可用模型 ID。',
]
