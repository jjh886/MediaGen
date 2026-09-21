import { documents, type ApiDocument } from './catalog'

export const errors = [
  [
    '400',
    'invalid_request / unsupported_parameter / invalid_previous_response',
    '参数、模型能力或续接上下文不匹配；查询模型能力后修正请求。',
  ],
  [
    '401',
    'invalid_api_key / authentication_error / UNAUTHORIZED',
    'API Key 无效或已撤销；确认 Authorization 请求头。',
  ],
  ['403', 'permission_error', '无接口或资源权限；检查账号及授权范围。'],
  [
    '404',
    'model_not_found / response_not_found / TASK_NOT_FOUND',
    '资源不存在，或不属于当前 API Key。',
  ],
  [
    '409',
    'account_unavailable / task_cancelled / invalid_state_error / QUEUE_ACTION_FAILED',
    '账号不可用、任务已结束或状态冲突；检查任务和恢复时间。',
  ],
  ['413 / 415', 'body_too_large / unsupported_media_type', '文件太大或 Content-Type 错误。'],
  [
    '429',
    'account_unavailable / rate_limit_exceeded / QUEUE_FULL',
    '并发、配额或队列已满；等待窗口恢复后重试，图片可改为排队提交。',
  ],
  [
    '500 / 502 / 503',
    'server_error / realtime_upstream_rejected / QUEUE_DISABLED / RESULT_STORAGE_ERROR',
    '上游、连接或存储暂时不可用；通过 x-request-id 排查。提交状态不明时不要盲目重放生成请求。',
  ],
]
export const guide = `# MediaGen 对外接口文档

默认 Base URL：http://127.0.0.1:18080。所有接口携带 Authorization: Bearer $MEDIAGEN_API_KEY。
请先 GET /v1/models；可用模型和参数由账号、插件及平台权限共同决定。
示例中的模型、任务 ID、文件名和尖括号内容需要替换为自己的真实数据；这些是格式示例，不是实际生成结果。
X-Client-Request-Id 可携带 1–512 个可打印 ASCII 字符；响应 x-request-id 用于排查请求。
图片为 Base64；TTS、文件、视频内容接口为二进制；实时音频为 JSON 事件中的 Base64。
async/sync、图片排队、Live、视频取消等是 MediaGen 扩展，非 OpenAI 官方字段或协议。

## 图片排队
POST /v1/images/generations 使用 async=true 或 sync=false，返回 HTTP 202 和 data.taskId。
使用同一 API Key GET /api/v2/generation-tasks/{task_id}；完成后的 data.result.data[].b64_json 是完整图片。
未配置回调也可取回结果；新生成的队列图片结果加密保存在本地，服务重启后仍能读取。旧版本未保存的图片无法追溯补回。
callback_url 可接收 {object:"generation.task.callback",event:"image_generation.completed",task_id,status:"completed",data:{created,data:[{b64_json:"..."}]}}；接收端应按 task_id 去重，失败回调会重试。

## 文本 SSE
在 Responses 或 Chat 请求中设置 stream=true，响应 Content-Type 为 text/event-stream。
Responses 示例：
event: response.output_text.delta
data: {"type":"response.output_text.delta","delta":"你好"}

event: response.completed
data: {"type":"response.completed","response":{"id":"resp_example","status":"completed","output":[]}}

Chat 使用 choices[].delta，最后 data: [DONE]；stream_options.include_usage=true 时在 [DONE] 前返回用量。
图片流返回 image_generation.completed（编辑为 image_edit.completed）；仅支持 partial_images 的模型提供中间预览。

## 实时音频输入
连接 /v1/realtime?model=gpt-realtime-2.1（Grok 为 grok-voice-latest）。
1. 发送 session.update 配置音频格式与声音，等待 session.updated。
2. 发送 {"type":"input_audio_buffer.append","audio":"<PCM 24kHz 单声道 16bit Base64>"}。
3. 手动模式发送 {"type":"input_audio_buffer.commit"}，再发 {"type":"response.create"}；server_vad 模式可自动断句与回答。
4. 收集 response.output_audio.delta 与 response.output_audio_transcript.delta，直到 response.done。
5. 可发送 response.cancel 中断，或关闭连接结束会话。工具结果通过 conversation.item.create 的 function_call_output 回传。
浏览器 WebSocket 不能直接设置 Authorization 请求头，建议由应用服务端转接；不要把长期 API Key 放入 URL。

## 常见错误
标准错误形状：{"error":{"message":"错误说明","type":"invalid_request_error","param":null,"code":"unsupported_parameter"}}。
排队接口保留原有 success/data 包装及大写错误码；业务状态请同时检查 HTTP 状态、status 和 error。
${errors.map(([status, code, help]) => `- HTTP ${status} · ${code}：${help}`).join('\n')}
`

export function requestExample(doc: ApiDocument): string {
  if (doc.method === 'WS')
    return `// Node.js，先安装 ws；API Key 保存在环境变量中\nimport WebSocket from 'ws'\nconst ws = new WebSocket('ws://127.0.0.1:18080${doc.path}', {\n  headers: { Authorization: 'Bearer ' + process.env.MEDIAGEN_API_KEY }\n})\nws.on('open', () => ws.send(JSON.stringify(${JSON.stringify(doc.body, null, 2)})))\nws.on('message', data => console.log(data.toString()))`
  const parts = [
    `curl -X ${doc.method} "$BASE_URL${doc.path}"`,
    '  -H "Authorization: Bearer $MEDIAGEN_API_KEY"',
  ]
  if (doc.form) parts.push(...doc.form.map((part) => `  -F '${part}'`))
  else if (doc.body)
    parts.push(
      '  -H "Content-Type: application/json"',
      `  -d '${JSON.stringify(doc.body, null, 2).replaceAll("'", "'\\''")}'`
    )
  if (doc.path === '/v1/audio/speech') parts.push('  --output speech.mp3')
  if (doc.path.endsWith('/content')) parts.push('  --output result.bin')
  return 'export BASE_URL="http://127.0.0.1:18080"\n' + parts.join(' \\\n')
}
export function fullDocumentation(): string {
  return (
    guide +
    '\n' +
    documents
      .map(
        (doc) =>
          `## ${doc.method} ${doc.path} · ${doc.title}\n\n${doc.description}\n\n${doc.parameters.map((p) => '- ' + p).join('\n')}\n\n请求示例：\n\`\`\`${doc.method === 'WS' ? 'javascript' : 'bash'}\n${requestExample(doc)}\n\`\`\`\n\n返回示例（HTTP ${doc.status || (doc.method === 'WS' ? 101 : 200)}）：\n\`\`\`\n${typeof doc.response === 'string' ? doc.response : JSON.stringify(doc.response, null, 2)}\n\`\`\`\n`
      )
      .join('\n')
  )
}
