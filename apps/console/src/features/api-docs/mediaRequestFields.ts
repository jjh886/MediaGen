import { fields, type ApiField } from './schema'

export function imageRequest(edit: boolean, variation: boolean): ApiField[] {
  return fields([
    [
      'model',
      'string',
      '否（建议填写）',
      'gpt-image-1',
      '图片模型 ID；默认模型不可用时返回 404，建议从模型目录选择。',
    ],
    ...(!variation
      ? ([['prompt', 'string', '是', '—', '图片描述；编辑时为修改指令。']] as const)
      : []),
    ['n', 'integer', '否', '1', '生成数量，取值 1–4，仍受模型上限约束。'],
    ['size', 'string', '否', 'auto', 'auto 或 宽x高；具体支持尺寸见模型目录。'],
    ['quality', 'string', '否', 'auto', 'auto / low / medium / high 等，以模型支持值为准。'],
    ['background', 'string', '否', 'auto', 'auto / transparent / opaque；透明背景不能使用 JPEG。'],
    ['response_format', 'string', '否', 'b64_json', '仅接受 b64_json，图片不返回 URL。'],
    ['output_format', 'string', '否', 'png', 'png / jpeg / webp。'],
    ['output_compression', 'integer', '否', '由编码器决定', '0–100；仅 JPEG / WebP 接受此字段。'],
    ['moderation', 'string', '否', '由模型决定', '内容审核等级；按模型支持值校验。'],
    [
      'reasoning_effort',
      'string',
      '否',
      '由模型决定',
      '思考深度；仅接受 capabilities.reasoning.efforts 声明的值，不支持时明确拒绝，不自动降档。',
    ],
    ['input_fidelity', 'string', '否', '由模型决定', '参考图保真度；仅模型声明支持时可用。'],
    ['stream', 'boolean', '否', 'false', '是否返回图片 SSE 事件。'],
    ['partial_images', 'integer', '否', '未设置', '0–3；大于 0 时须 stream=true 且模型支持预览。'],
    ['user', 'string', '否', '未设置', '终端用户标识。'],
    [
      'image_aspect_ratio',
      'string',
      '否',
      '按 size 推导',
      'MediaGen 扩展宽高比；auto 尺寸默认 auto。',
    ],
    ...(edit
      ? ([
          [
            'image / image[]',
            'file / file[]',
            '条件必填',
            '—',
            'multipart 参考图，至少一张；与 JSON images 方式二选一。',
          ],
          [
            'images',
            'array<object>',
            '条件必填',
            '[]',
            'JSON 参考图数组；与 multipart image 方式二选一。',
          ],
          [
            'images[].image_url',
            'string',
            '每张图二选一',
            '—',
            '图片 Data URL；远程 URL 是否接受取决于模型。',
          ],
          ['images[].file_id', 'string', '每张图二选一', '—', '同一 API Key 上传的图片文件 ID。'],
          [
            'mask',
            'file | object',
            '否',
            '未设置',
            'multipart 遮罩文件，或含 image_url/file_id 的 JSON 对象；须模型支持。',
          ],
        ] as [string, string, string, string, string][])
      : ([
          ['async', 'boolean', '否', 'false', '扩展：true 使用持久化图片队列，返回 HTTP 202。'],
          [
            'sync',
            'boolean',
            '否',
            '未设置（同步）',
            '扩展：false 等价于 async=true；true 与 async=true 冲突。',
          ],
          ['callback_url', 'string', '否', '未设置', '扩展：异步完成回调地址，仅排队模式有效。'],
          ['webhook_url', 'string', '否', '未设置', 'callback_url 的兼容别名，二者不要同时传。'],
        ] as [string, string, string, string, string][])),
  ])
}
export const speechRequest = fields([
  ['model', 'string', '是', '—', '支持 audio_speech 的模型 ID。'],
  ['input', 'string', '是', '—', '朗读文本，1–4096 个字符。'],
  ['voice', 'string', '是', '—', '声音 ID，可选值见模型 capabilities.voices。'],
  [
    'response_format',
    'string',
    '否',
    'mp3',
    'mp3 / wav / pcm / flac / aac / opus，以模型支持值为准。',
  ],
  ['speed', 'number', '否', '1', '语速；取值范围见模型 capabilities.speed。'],
  ['instructions', 'string', '否', '未设置', '语音风格指令，仅支持的模型可用。'],
  ['stream_format', 'string', '否', 'audio', '当前仅接受 audio；不支持 SSE。'],
  ['stream', 'boolean', '否', 'false', '按模型能力接受；此接口成功响应始终为音频二进制。'],
])
export const transcriptionRequest = fields([
  ['model', 'string', '是', '—', '转写/翻译模型 ID；翻译需要模型声明 translate_audio。'],
  ['file', 'file', '是', '—', 'multipart 上传的音频；整个上传内容不能超过 25 MiB。'],
  ['response_format', 'string', '否', 'json', '当前 json 返回 {text}；text 返回纯文本。'],
  ['language', 'string', '否', '由模型识别', '音频语种，仅支持的模型可用。'],
  ['prompt', 'string', '否', '未设置', '上下文提示，仅支持的模型可用。'],
  ['temperature', 'number（表单字符串）', '否', '由模型决定', '采样温度，仅支持的模型可用。'],
])
export const videoRequest = fields([
  ['model', 'string', '是', '—', '支持 video 的模型 ID。'],
  ['prompt', 'string', '是', '—', '视频描述，最长 32000 字符。'],
  [
    'seconds',
    'integer | string',
    '否',
    '插件默认值，否则 4',
    '1–60 的正整数或数字字符串；实际范围受模型约束。',
  ],
  [
    'size',
    'string',
    '否',
    '插件默认值，否则 720x1280',
    '宽x高；取值见模型能力。豆包默认 1280x720，按尺寸推导比例，不保证精确输出像素。',
  ],
  ['quality', 'string', '否', 'standard', '清晰度等级，按模型能力校验。'],
  [
    'input_reference',
    'file | object',
    '否',
    '未设置',
    '参考图：multipart 文件，或 JSON {image_url} / {file_id}。',
  ],
  [
    'input_reference.image_url',
    'string',
    'JSON 引用二选一',
    '—',
    '参考图 Data URL 或模型允许的远程 URL。',
  ],
  ['input_reference.file_id', 'string', 'JSON 引用二选一', '—', '同一 API Key 上传的文件 ID。'],
  [
    'input_references',
    'array<object>',
    '否',
    '[]',
    'MediaGen 多参考图扩展，每项结构同 input_reference。',
  ],
  [
    'video_reference',
    'object（JSON）',
    '否',
    '未设置',
    'MediaGen 参考视频扩展；豆包 Seedance 2.0 Fast 支持最多 1 个 MP4/MOV。',
  ],
  [
    'video_reference.file_id',
    'string',
    '视频来源二选一',
    '—',
    '同一 API Key 经 /v1/files 上传的参考视频 ID。',
  ],
  [
    'video_reference.file_data',
    'string',
    '视频来源二选一',
    '—',
    '视频 Data URL，或搭配 filename 的 Base64；视频创建请求体上限 50 MiB。',
  ],
  [
    'video_reference.filename',
    'string',
    '裸 Base64 时必填',
    '未设置',
    '包含 .mp4 或 .mov 扩展名的文件名；不接受服务器本地路径。',
  ],
  ['user', 'string', '否', '未设置', '终端用户标识。'],
])
