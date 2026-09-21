import { type ApiExample } from './samples'
export const audioDocs: ApiExample[] = [
  {
    group: '音频',
    method: 'POST',
    path: '/v1/audio/speech',
    title: '文字转语音 TTS',
    description: '成功返回音频二进制，不是 JSON 或下载 URL。可使用 curl --output 保存。',
    notes: [
      'model、input、voice 必填；支持的声音以模型目录及插件为准。',
      'response_format：mp3/wav/pcm/flac/aac/opus，依模型能力；PCM 为单声道 24kHz 16bit。',
      'speed、instructions：依模型能力；stream_format 当前仅 audio，不支持 SSE。',
    ],
    body: {
      model: 'grok-tts',
      input: '你好，欢迎使用 MediaGen。',
      voice: 'eve',
      response_format: 'mp3',
      speed: 1,
    },
    response: 'HTTP 200 · Content-Type: audio/mpeg\n<MP3 二进制音频>',
  },
  ...(['transcriptions', 'translations'] as const).map(
    (kind): ApiExample => ({
      group: '音频',
      method: 'POST',
      path: `/v1/audio/${kind}`,
      title: kind === 'transcriptions' ? '语音转写' : '音频翻译为英文',
      description:
        'multipart/form-data 文件上传；是否支持翻译及参数，以模型能力为准。Grok 转写通道未声明翻译能力时返回 400。',
      notes: [
        'model、file 必填；文件最大 25 MiB。',
        'language：输入语种；response_format 当前 json / text；prompt、temperature、时间戳等仅声明支持的模型接受。',
      ],
      form: ['model=chatgpt-transcribe', 'file=@speech.wav', 'response_format=json'],
      response: { text: kind === 'transcriptions' ? '你好' : 'Hello' },
    })
  ),
  {
    group: '实时语音',
    method: 'WS',
    path: '/v1/realtime?model=gpt-realtime-2.1',
    title: '实时双向语音',
    description:
      'Codex 使用 gpt-realtime-2.1，Grok 使用 grok-voice-latest。音频片段在 JSON 事件内以 Base64 传输。',
    notes: [
      '握手请求头 Authorization: Bearer API_KEY；model 为查询参数。',
      'session.update：配置 instructions、output_modalities、audio.input/output、tools、tool_choice。',
      'input_audio_buffer.append：audio 为 PCM Base64；手动模式需 commit + response.create；server_vad 可自动断句。',
      'conversation.item.create 支持文本和工具结果；response.create 发起回答；response.cancel 取消生成。关闭 WebSocket 结束通话并释放账号。',
      '输出事件：response.output_audio.delta、response.output_audio_transcript.delta、response.done；错误通过 error 事件返回。',
    ],
    body: {
      type: 'session.update',
      session: {
        type: 'realtime',
        output_modalities: ['audio'],
        audio: {
          input: {
            format: { type: 'audio/pcm', rate: 24000 },
            turn_detection: { type: 'server_vad' },
          },
          output: { format: { type: 'audio/pcm', rate: 24000 }, voice: 'marin' },
        },
      },
    },
    response: { type: 'response.output_audio.delta', delta: '<PCM Base64>' },
  },
  ...(['/v1/live/sessions', '/v1/live'] as const).map(
    (path): ApiExample => ({
      group: '实时语音',
      method: 'POST',
      path,
      title: 'Live WebRTC 会话（扩展）',
      description:
        '私有 Live 通道，需要模型声明的设备环境与权限；普通实时语音请使用 /v1/realtime。两条路径为同一创建能力的别名。',
      notes: [
        'session.model=gpt-live-1；transport.type=webrtc；transport.sdp 为浏览器生成的完整 SDP offer。',
      ],
      body: {
        session: { model: 'gpt-live-1' },
        transport: { type: 'webrtc', sdp: '<完整 SDP offer>' },
      },
      response: {
        session: { id: 'call_example' },
        transport: { type: 'webrtc', sdp: '<SDP answer>' },
      },
    })
  ),
  ...(['/v1/live/sessions/{call_id}/attach', '/v1/live/{call_id}'] as const).map(
    (path): ApiExample => ({
      group: '实时语音',
      method: 'WS',
      path,
      title: 'Live 辅助事件连接（扩展）',
      description: '只能连接同一 API Key 已创建的 Live 会话；不能用来替代主 WebRTC 媒体连接。',
      notes: ['call_id：Live 创建返回的 session.id；需要 Bearer 鉴权。'],
      body: { type: 'session.update', session: { instructions: '请简短回答。' } },
      response: { type: 'session.updated' },
    })
  ),
  {
    group: '实时语音',
    method: 'POST',
    path: '/v1/live/sessions/{call_id}/hangup',
    title: '挂断 Live 会话（扩展）',
    description: '结束该 Key 拥有的会话，释放账号。',
    notes: ['call_id：会话 ID。'],
    body: {},
    response: 'HTTP 200 · 空响应体',
  },
]
