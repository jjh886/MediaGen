import { fields, output } from './schema'

export const realtimeRequest = fields(
  [['model', 'string', '是', '—', '实时模型 ID，例如 gpt-realtime-2.1 或 grok-voice-latest。']],
  'query'
).concat(
  fields(
    [
      [
        'type',
        'string',
        '是',
        '—',
        '事件类型：session.update、input_audio_buffer.append/commit/clear、conversation.item.create、response.create、response.cancel。',
      ],
      ['event_id', 'string', '否', '未设置', '调用方事件标识，用于关联错误。'],
      ['session', 'object', 'session.update 时必填', '—', '需要更新的会话配置，支持项由模型决定。'],
      ['session.type', 'string', '模型要求时', 'realtime', '实时会话类型。'],
      ['session.instructions', 'string', '否', '由模型决定', '系统指令。'],
      [
        'session.output_modalities',
        'array<string>',
        '否',
        '由模型决定',
        '期望的输出模态，例如 ["audio"]。',
      ],
      ['session.audio', 'object', '音频配置时', '由模型决定', '输入和输出音频设置。'],
      [
        'session.audio.input.format',
        'object',
        '指定输入格式时',
        '由模型决定',
        '例如 {type:"audio/pcm",rate:24000}。',
      ],
      [
        'session.audio.input.turn_detection',
        'object | null',
        '否',
        '由模型决定',
        '{type:"server_vad"} 自动断句；null 使用手动 commit + response.create。',
      ],
      [
        'session.audio.input.transcription',
        'object',
        '否',
        '由模型决定',
        '输入音频转写配置，仅支持此能力的模型接受。',
      ],
      [
        'session.audio.output.format',
        'object',
        '指定输出格式时',
        '由模型决定',
        '例如 {type:"audio/pcm",rate:24000}。',
      ],
      [
        'session.audio.output.voice',
        'string',
        '否',
        '由模型决定',
        '声音 ID，查询模型支持值；Codex 示例为 marin。',
      ],
      [
        'session.tools',
        'array<object>',
        '否',
        '未设置',
        '工具声明，每项包括 type、name、description、parameters。',
      ],
      ['session.tool_choice', 'string | object', '否', '由模型决定', '工具选择策略。'],
      ['audio', 'string', 'append 事件必填', '—', '音频 Base64；PCM 示例为 24kHz、单声道、16bit。'],
      ['item', 'object', 'conversation.item.create 必填', '—', '消息或工具结果。'],
      ['item.type', 'string', 'item 存在时', '—', 'message 或 function_call_output。'],
      ['item.role', 'string', '消息项必填', '—', '例如 user。'],
      [
        'item.content',
        'array<object>',
        '消息项必填',
        '—',
        '文本消息示例：[{type:"input_text",text:"你好"}]。',
      ],
      ['item.call_id', 'string', '工具结果必填', '—', '对应上游工具调用的 call_id。'],
      ['item.output', 'string', '工具结果必填', '—', '工具结果文本或 JSON 字符串。'],
      [
        'response',
        'object',
        '否',
        '继承会话设置',
        'response.create 的单次生成配置，例如 instructions。',
      ],
      ['response_id', 'string', '否', '当前生成', 'response.cancel 可指定要取消的响应。'],
    ],
    'event'
  )
)
export const realtimeResponse = output([
  [
    'type',
    'string',
    '是',
    '—',
    'session.created/updated、response.output_audio.delta、response.output_audio_transcript.delta、response.done、error 等。',
  ],
  ['event_id', 'string', '条件返回', '—', '服务端事件 ID。'],
  ['session', 'object', '会话事件返回', '—', '会话配置，具体字段由上游提供。'],
  ['session.id', 'string', '会话事件返回', '—', '会话 ID。'],
  ['response_id', 'string', '响应增量时', '—', '所属响应 ID。'],
  ['item_id', 'string', '内容事件时', '—', '所属输出项 ID。'],
  ['output_index', 'integer', '条件返回', '—', '输出项索引。'],
  ['content_index', 'integer', '条件返回', '—', '内容块索引。'],
  ['delta', 'string', '增量事件返回', '—', '音频事件为 Base64；转录事件为新增文字。按 type 区分。'],
  ['transcript', 'string', '转录完成事件', '—', '完整转录文本。'],
  ['response', 'object', 'response.done 返回', '—', '最终响应与用量。'],
  ['response.id', 'string', 'response.done 返回', '—', '响应 ID。'],
  [
    'response.status',
    'string',
    'response.done 返回',
    '—',
    'completed / cancelled / failed / incomplete 等。',
  ],
  [
    'response.output',
    'array<object>',
    'response.done 返回',
    '—',
    '输出消息或工具调用；工具调用包含 name、arguments、call_id。',
  ],
  ['response.usage', 'object', '上游提供时', '—', 'token 用量及音频/文本细分。'],
  [
    'error',
    'object',
    'error 事件返回',
    '—',
    '错误对象，包含 type、code、message，可能包含 param、event_id。',
  ],
])
export const liveRequest = fields([
  ['session', 'object', '是', '—', 'Live 会话配置。'],
  ['session.model', 'string', '是', '—', '具备 Live WebRTC 能力的模型，如 gpt-live-1。'],
  ['transport', 'object', '是', '—', 'WebRTC 传输配置。'],
  ['transport.type', 'string', '是', 'webrtc', '当前 Live 接口仅支持 webrtc。'],
  ['transport.sdp', 'string', '是', '—', '浏览器生成的完整 SDP offer，必须以 v=0 开头。'],
])
export const liveResponse = output([
  ['session', 'object', '是', '—', '已创建的 Live 会话。'],
  ['session.id', 'string', '是', '—', 'call_id，用于 attach 和 hangup。'],
  ['transport', 'object', '是', '—', 'WebRTC 传输结果。'],
  ['transport.type', 'string', '是', 'webrtc', '传输类型。'],
  ['transport.sdp', 'string', '是', '—', 'SDP answer，交给浏览器 setRemoteDescription。'],
])
