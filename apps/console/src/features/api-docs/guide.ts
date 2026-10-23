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
        text: '示例中的模型、ID、文件名和尖括号内容需替换为真实数据。async / sync、生成排队、Live 和视频取消属于 MediaGen 扩展，不是 OpenAI 官方字段或协议。',
      },
    ],
  },
  {
    id: 'video',
    title: '豆包视频',
    description: '文生视频、参考图和参考视频共用 /v1/videos，完成后下载 MP4。',
    blocks: [
      {
        kind: 'table',
        title: '豆包视频模型与参考输入',
        headings: ['模型 ID', '显示名称', '输入'],
        rows: [
          ['doubao-seedance-2-5', 'Seedance 2.5', '文本、图片'],
          ['doubao-seedance-2', 'Seedance 2.0', '文本、图片'],
          ['doubao-seedance-2-fast', 'Seedance 2.0 Fast', '文本、图片、参考视频'],
          ['doubao-seedance-2-mini', 'Seedance 2.0 Mini', '文本、图片'],
        ],
      },
      {
        kind: 'steps',
        items: [
          {
            title: '准备账号和素材',
            text: '在控制台登录豆包，授权 API Key 使用账号池。参考视频先通过 `POST /v1/files` 上传；参考图也可在创建时用 multipart 上传。',
          },
          {
            title: '创建视频',
            text: '调用 `POST /v1/videos`，指定具体模型和 prompt。默认 10 秒、1280x720；不要添加 async、sync 或 stream。HTTP 200 返回 `video_*` 任务 ID。',
          },
          {
            title: '等待任务完成',
            text: '使用同一 Key 轮询 `GET /v1/videos/{video_id}`。queued / in_progress 继续等待，failed 查看 error；如需账号验证，在控制台完成验证。',
          },
          {
            title: '下载成品',
            text: 'completed 后调用 `GET /v1/videos/{video_id}/content` 并保存为 .mp4；有封面时可用 `?variant=thumbnail` 下载。',
          },
        ],
      },
      {
        kind: 'note',
        text: '模型权限、时长与参考素材限制会由插件按当前账号校验，不自动降级。微信提醒需事先在控制台完成绑定和通知配置，接口请求本身不会建立绑定。',
      },
    ],
  },
  {
    id: 'conversation-title',
    title: '会话名称',
    description: 'ChatGPT、豆包和 DeepSeek 自动读取平台会话标题，通过 metadata 返回。',
    blocks: [
      {
        kind: 'table',
        title: '会话名称的读取位置',
        headings: ['调用方式', '读取位置', '返回时机'],
        rows: [
          ['Responses JSON', 'metadata.conversation_title', '请求完成后的响应'],
          [
            'Responses SSE / WebSocket',
            'response.metadata.conversation_title',
            'response.completed 事件',
          ],
          ['Chat JSON', 'metadata.conversation_title', '请求完成后的响应，MediaGen 扩展'],
          [
            'Chat SSE',
            'metadata.conversation_title',
            'finish_reason 非空的结束帧，位于 [DONE] 之前',
          ],
          [
            '查询已保存响应',
            'metadata.conversation_title',
            'GET /v1/responses/{response_id}，须 store=true',
          ],
        ],
      },
      {
        kind: 'code',
        title: '请求示例 · POST /v1/responses',
        language: 'json',
        value: JSON.stringify(
          {
            model: 'doubao-text',
            input: '帮我设计橙色杯子的广告文案',
            metadata: { project: 'cup' },
            store: true,
          },
          null,
          2
        ),
      },
      {
        kind: 'code',
        title: '完成响应中的元数据示例',
        language: 'json',
        value: JSON.stringify(
          {
            metadata: {
              project: 'cup',
              conversation_id: 'upstream-conversation-id',
              conversation_title: '橙色杯子广告',
            },
          },
          null,
          2
        ),
      },
      {
        kind: 'code',
        title: 'Chat 流式结束帧示例',
        language: 'text',
        value:
          'data: {"id":"chatcmpl_example","object":"chat.completion.chunk","created":1790000000,"model":"doubao-text","choices":[{"index":0,"delta":{},"finish_reason":"stop","logprobs":null}],"metadata":{"conversation_id":"upstream-conversation-id","conversation_title":"橙色杯子广告"}}\n\ndata: [DONE]',
      },
      {
        kind: 'note',
        text: '无需额外参数，也不会额外调用模型生成标题。最多读取 3 次，每次请求超时 3 秒；默认标题、空标题或读取失败时省略标题字段，正文仍正常返回。标题最多 120 个 Unicode 字符；Codex 当前不提供此平台标题。请求 metadata 会保留，同名冲突时插件字段优先。conversation_id 是上游 ID，不能代替 previous_response_id。',
      },
    ],
  },
  {
    id: 'queue',
    title: '图片与文本排队',
    description:
      '先提交任务，再查询结果。图片和文本独立排队、互不占用并发，不能同时开启 stream=true。',
    blocks: [
      {
        kind: 'steps',
        items: [
          {
            title: '提交生成任务',
            text: '图片调用 `POST /v1/images/generations`，设置 `sync=false` 或 `async=true`；文本调用 `POST /v1/responses` 或 `POST /v1/chat/completions`，仅设置 `async=true`。省略时均为同步。',
          },
          {
            title: '保存任务 ID',
            text: '接口返回 HTTP 202，取出 `data.taskId`。此时任务尚未生成完成。',
          },
          {
            title: '查询生成状态',
            text: '使用同一 API Key 调用 `GET /api/v2/generation-tasks/{task_id}`。排队或处理中时继续等待。',
          },
          {
            title: '读取生成结果',
            text: '当 `data.status=completed` 时，图片读取 `data.result.data[].b64_json`，文本读取 `data.result.output`（Responses）或 `data.result.choices`（Chat）。失败或取消时查看 `data.failureReason`。',
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
        title: '提交文本任务 · JSON 请求体',
        language: 'json',
        value: JSON.stringify(
          {
            model: 'gpt-6-luna',
            input: '请总结剧本。',
            async: true,
            callback_url: 'https://your-app.example/callback',
          },
          null,
          2
        ),
      },
      {
        kind: 'paragraph',
        text: '使用 `GET /api/v2/generation-queue?taskKind=text` 查询文本队列，`taskKind=image` 查询图片队列。管理台生成队列可切换类型，分别插队、取消；文本并发默认由插件提供（仅 ChatGPT 为 3，其余为 0，表示不设本地文本并发上限），可在账号配置调整，填 0 使用默认值。',
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
        text: '图片与文本异步任务均可设置 `callback_url`，终态后向该地址 POST JSON。文本事件为 `text_generation.completed`、`text_generation.failed`、`text_generation.cancelled`，图片使用 `image_generation` 前缀。完成结果在 `data` 中，失败或取消原因在 `error` 中。接收端保存结果后返回 2xx，并按 `task_id` 去重。每次回调超时 15 秒，失败最多尝试 3 次，间隔 1 秒、2 秒；服务重启不补发通知，通知失败不改变生成结果，可查询兜底。',
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
        text: '不配置回调也能通过查询接口取回结果。新生成的队列结果加密保存在本地，服务重启后仍可读取；旧版本未保存的结果不能追溯补回。',
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
