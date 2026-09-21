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
  '字段表描述当前网关的协议。标为“由模型决定”或“模型支持时”的参数，需要先查询 /v1/models；不支持的参数返回 400。',
  'HTTP Responses 默认保存响应；Chat 不支持 store=true；WebSocket 的本地续接默认仅在当前连接内有效。',
  '图片、文件输入和原生工具调用按模型能力开放；示例展示请求结构，不代表所有模型均支持全部字段。',
]
export const imageNotes = [
  '具体尺寸、质量、参考图数量等限制以模型能力为准。模型未声明的能力不会自动实现。',
  '图片一律返回 Base64；排队只支持图片生成，且不能与 stream=true 同时使用。',
  '参数表中的默认模型可能未安装或未授权，建议始终显式填写 /v1/models 返回的可用模型 ID。',
]
