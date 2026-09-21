export type GuideBlock =
  | { kind: 'paragraph' | 'note'; text: string }
  | { kind: 'table'; title: string; headings: string[]; rows: string[][] }
  | { kind: 'steps'; items: { title: string; text: string }[] }
  | { kind: 'code'; title: string; language: string; value: string }

export interface GuideSection {
  id: string
  title: string
  description: string
  blocks: GuideBlock[]
}

export const guideSections: GuideSection[] = [
  {
    id: 'conventions',
    title: '调用约定',
    description: '先确认模型能力和鉴权方式，再选择对应的生成接口。',
    blocks: [
      {
        kind: 'table',
        title: '请求与响应约定',
        headings: ['项目', '约定', '使用说明'],
        rows: [
          ['服务地址', 'http://127.0.0.1:18080', '本机默认地址；部署到其他机器时替换为实际地址。'],
          ['鉴权', 'Authorization: Bearer API_KEY', '所有对外接口都需要 API Key。'],
          [
            '模型发现',
            'GET /v1/models',
            '查看可用模型、输入类型和参数限制；能力由账号、插件及平台权限共同决定。',
          ],
          [
            '请求追踪',
            'X-Client-Request-Id',
            '可选，1–512 个可打印 ASCII 字符；响应 x-request-id 用于排查。',
          ],
        ],
      },
      {
        kind: 'code',
        title: '鉴权请求头示例',
        language: 'http',
        value: 'Authorization: Bearer <你的 API Key>\nX-Client-Request-Id: my-request-001',
      },
      {
        kind: 'table',
        title: '返回内容如何读取',
        headings: ['内容', '返回格式', '读取方式'],
        rows: [
          ['图片', 'Base64', '读取 b64_json 并解码为图片，不返回下载 URL。'],
          ['TTS / 文件 / 视频内容', '二进制', '按 Content-Type 保存或播放，不按 JSON 解析。'],
          ['实时音频', 'JSON 事件中的 Base64', '按事件顺序解码音频片段并播放。'],
        ],
      },
      {
        kind: 'note',
        text: '示例中的模型、ID、文件名和尖括号内容需替换为真实数据。async / sync、图片排队、Live 和视频取消属于 MediaGen 扩展，不是 OpenAI 官方字段或协议。',
      },
    ],
  },
  {
    id: 'queue',
    title: '图片排队',
    description: '先提交任务，再查询结果。排队只适用于图片生成，不能同时开启 stream=true。',
    blocks: [
      {
        kind: 'steps',
        items: [
          {
            title: '提交图片任务',
            text: '调用 `POST /v1/images/generations`，设置 `sync=false` 或 `async=true`。',
          },
          {
            title: '保存任务 ID',
            text: '接口返回 HTTP 202，取出 `data.taskId`。此时图片尚未生成完成。',
          },
          {
            title: '查询生成状态',
            text: '使用同一 API Key 调用 `GET /api/v2/generation-tasks/{task_id}`。排队或处理中时继续等待。',
          },
          {
            title: '读取最终图片',
            text: '当 `data.status=completed` 时，读取 `data.result.data[].b64_json`。失败或取消时查看 `data.failureReason`。',
          },
        ],
      },
      {
        kind: 'code',
        title: '提交任务 · JSON 请求体',
        language: 'json',
        value: JSON.stringify(
          { model: 'gpt-image-2', prompt: '一只窗边的猫', sync: false },
          null,
          2
        ),
      },
      {
        kind: 'code',
        title: '入队成功 · HTTP 202',
        language: 'json',
        value: JSON.stringify(
          {
            success: true,
            data: { taskId: 'task-example', status: 'queued', message: '开始排队生成中' },
          },
          null,
          2
        ),
      },
      {
        kind: 'paragraph',
        text: '也可以在提交任务时设置 `callback_url`，完成后接收以下回调。接收端应按 `task_id` 去重，回调失败会重试。',
      },
      {
        kind: 'code',
        title: '完成回调 · JSON 结构示例',
        language: 'json',
        value: JSON.stringify(
          {
            object: 'generation.task.callback',
            event: 'image_generation.completed',
            task_id: 'task-example',
            status: 'completed',
            data: { created: 1790000000, data: [{ b64_json: '<完整图片 Base64>' }] },
          },
          null,
          2
        ),
      },
      {
        kind: 'note',
        text: '不配置回调也能通过查询接口取回结果。新生成的队列图片加密保存在本地，服务重启后仍可读取；旧版本未保存的图片不能追溯补回。',
      },
    ],
  },
  {
    id: 'stream',
    title: '流式输出',
    description:
      '设置 stream=true 后按事件逐步读取。SSE 的响应类型为 text/event-stream，不是单个 JSON。',
    blocks: [
      {
        kind: 'table',
        title: '不同接口的流式读取方式',
        headings: ['接口', '读取内容', '结束标志'],
        rows: [
          [
            'Responses',
            'response.output_text.delta 的 delta 文本',
            'response.completed；失败时检查错误事件。',
          ],
          ['Chat Completions', 'choices[].delta 中的增量', 'data: [DONE]。'],
          [
            '图片生成 / 编辑',
            '图片事件中的 Base64',
            'image_generation.completed / image_edit.completed。',
          ],
        ],
      },
      {
        kind: 'code',
        title: 'Responses · SSE 事件示例',
        language: 'text',
        value:
          'event: response.output_text.delta\ndata: {"type":"response.output_text.delta","delta":"你好"}\n\nevent: response.completed\ndata: {"type":"response.completed","response":{"id":"resp_example","status":"completed","output":[]}}\n',
      },
      {
        kind: 'paragraph',
        text: 'Chat 设置 `stream_options.include_usage=true` 后，会在 `[DONE]` 前返回用量帧。图片只有在模型支持 `partial_images` 时才提供中间预览。',
      },
      {
        kind: 'note',
        text: 'SSE 事件以空行分隔。不要将一次网络读取当作一条完整事件；应按 SSE 格式解析后，再读取 data 中的 JSON。',
      },
    ],
  },
  {
    id: 'realtime',
    title: '实时语音',
    description: '通过 WebSocket 双向交换 JSON 事件，音频片段放在事件的 Base64 字段中。',
    blocks: [
      {
        kind: 'table',
        title: '选择实时模型',
        headings: ['通道', '连接路径'],
        rows: [
          ['Codex', '/v1/realtime?model=gpt-realtime-2.1'],
          ['Grok', '/v1/realtime?model=grok-voice-latest'],
        ],
      },
      {
        kind: 'steps',
        items: [
          {
            title: '配置会话',
            text: '发送 `session.update` 设置音频格式和声音，等待 `session.updated`。具体字段见下方实时语音接口参数表。',
          },
          {
            title: '持续发送音频',
            text: '发送 `input_audio_buffer.append` 事件。以下示例使用 24kHz、单声道、16bit PCM 的 Base64 编码。',
          },
          {
            title: '结束输入并请求回答',
            text: '手动模式依次发送 `input_audio_buffer.commit` 和 `response.create`。使用 `server_vad` 时，可由服务端自动断句与回答。',
          },
          {
            title: '接收音频和文字',
            text: '读取 `response.output_audio.delta` 与 `response.output_audio_transcript.delta`，直到 `response.done`。',
          },
          {
            title: '中断或结束会话',
            text: '发送 `response.cancel` 中断生成；关闭 WebSocket 结束会话。工具结果通过 `conversation.item.create` 的 `function_call_output` 回传。',
          },
        ],
      },
      {
        kind: 'code',
        title: '发送音频片段 · 单条 WebSocket 消息',
        language: 'json',
        value: JSON.stringify(
          { type: 'input_audio_buffer.append', audio: '<PCM 24kHz 单声道 16bit Base64>' },
          null,
          2
        ),
      },
      {
        kind: 'code',
        title: '手动结束输入 · 先发送',
        language: 'json',
        value: JSON.stringify({ type: 'input_audio_buffer.commit' }, null, 2),
      },
      {
        kind: 'code',
        title: '请求回答 · 再发送',
        language: 'json',
        value: JSON.stringify({ type: 'response.create' }, null, 2),
      },
      {
        kind: 'note',
        text: '浏览器 WebSocket 不能直接设置 Authorization 请求头，建议由应用服务端转接。不要把长期 API Key 放入 URL。',
      },
    ],
  },
  {
    id: 'errors',
    title: '错误处理',
    description: '先看 HTTP 状态，再读取错误对象；流式调用还要检查流中的失败事件。',
    blocks: [
      {
        kind: 'code',
        title: 'OpenAI 兼容接口 · 错误响应示例',
        language: 'json',
        value: JSON.stringify(
          {
            error: {
              message: '错误说明',
              type: 'invalid_request_error',
              param: null,
              code: 'unsupported_parameter',
            },
          },
          null,
          2
        ),
      },
      {
        kind: 'paragraph',
        text: '排队接口保留 `success/data` 包装和大写错误码。请同时检查 HTTP 状态、任务 `status` 及 `error`，不能只凭 HTTP 200 判断生成成功。',
      },
      {
        kind: 'note',
        text: '具体错误码及处理方式见页面下方“错误响应”。提交状态不明时，先查询任务或使用 x-request-id 排查，不要盲目重复提交生成请求。',
      },
    ],
  },
]

export function guideMarkdown(): string {
  const blockMarkdown = (block: GuideBlock): string => {
    switch (block.kind) {
      case 'paragraph':
        return block.text
      case 'note':
        return '> ' + block.text
      case 'code':
        return `### ${block.title}\n\n\`\`\`${block.language}\n${block.value}\n\`\`\``
      case 'steps':
        return block.items.map((item, i) => `${i + 1}. **${item.title}**：${item.text}`).join('\n')
      case 'table':
        return (
          `### ${block.title}\n\n` +
          [block.headings, block.headings.map(() => '---'), ...block.rows]
            .map((row) => '| ' + row.map((cell) => cell.replaceAll('|', '\\|')).join(' | ') + ' |')
            .join('\n')
        )
    }
  }
  return (
    '# MediaGen 对外接口文档\n\n' +
    guideSections
      .map(
        (section) =>
          `## ${section.title}\n\n${section.description}\n\n${section.blocks.map(blockMarkdown).join('\n\n')}`
      )
      .join('\n\n') +
    '\n'
  )
}
