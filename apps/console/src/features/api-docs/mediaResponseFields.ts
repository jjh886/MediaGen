import { output, prefix } from './schema'
import { usageFields } from './textResponseFields'

export const imageResponse = output([
  ['created', 'integer', '是', '—', '生成时间，Unix 秒。'],
  ['data', 'array<object>', '是', '—', '图片结果数组。'],
  [
    'data[].b64_json',
    'string',
    '是',
    '—',
    '完整图片 Base64；解码后得到图片文件，不含 data: 前缀。',
  ],
  ['data[].revised_prompt', 'string', '上游提供时', '—', '上游修订的提示词。'],
  ['output_format', 'string', '是', '—', '图片实际格式：png / jpeg / webp。'],
  ['size', 'string', '条件返回', '—', '实际图片尺寸。'],
  ['quality', 'string', '条件返回', '—', '实际或请求的质量等级。'],
  ['background', 'string', '条件返回', '—', '实际或请求的背景类型。'],
]).concat(usageFields)
export const imageStream = output([
  [
    'type',
    'string',
    '是',
    '—',
    'image_generation.partial_image / image_generation.completed；编辑接口前缀为 image_edit。',
  ],
  [
    'b64_json',
    'string',
    '图片事件返回',
    '—',
    '本帧完整预览图或最终图片 Base64；不要拼接多个 Base64 字符串。',
  ],
  ['partial_image_index', 'integer', '预览事件返回', '—', '预览图序号。'],
  ['created_at', 'integer', '条件返回', '—', '事件时间，Unix 秒。'],
  ['output_format', 'string', '条件返回', '—', '当前图片格式。'],
  ['size', 'string', '条件返回', '—', '当前图片尺寸。'],
  ['usage', 'object', '完成且上游提供时', '—', '图片 token 用量；子字段同非流式 usage。'],
])
export const queueAccepted = output([
  ['success', 'boolean', '是', 'true', '入队成功标志，HTTP 状态为 202。'],
  ['data.taskId', 'string', '是', '—', '任务 ID，供查询、取消或批量查询。'],
  ['data.status', 'string', '是', 'queued', '当前已进入队列。'],
  ['data.message', 'string', '是', '—', '入队说明。'],
])
export const queueItem = output([
  ['taskId', 'string', '是', '—', '生成任务 ID。'],
  ['platform', 'string', '是', '—', '执行平台。'],
  ['taskKind', 'string', '是', '—', 'image 或 text；分别属于图片和文本队列。'],
  ['modelId', 'string', '是', '—', '请求的生成模型。'],
  ['prompt', 'string', '是', '—', '请求摘要；文本任务可能为空。'],
  ['status', 'string', '是', 'queued', '队列列表只包含等待执行的任务。'],
  ['position', 'integer', '是', '—', '在当前调用者同类型等待队列中的位置，从 1 开始。'],
  ['queuedAt', 'string', '是', '—', '入队时间，RFC 3339。'],
])
export const taskFields = output([
  ['taskId', 'string', '是', '—', '生成任务 ID。'],
  ['platform', 'string', '是', '—', '执行平台。'],
  ['taskKind', 'string', '是', '—', 'image 或 text；分别属于图片和文本队列。'],
  ['modelId', 'string', '是', '—', '生成模型 ID。'],
  ['prompt', 'string', '是', '—', '请求摘要；文本任务可能为空。'],
  ['status', 'string', '是', '—', 'queued / processing / completed / failed / cancelled。'],
  ['position', 'integer | null', '是', 'null（未排队）', '等待队列位置，从 1 开始。'],
  [
    'result',
    'object | null',
    '是',
    'null（未完成）',
    '完成后包含图片、Responses 或 Chat 原始响应；旧版本任务可能仅有摘要。',
  ],
  [
    'result.output',
    'array<object>',
    'Responses 文本完成时',
    '—',
    '模型输出，结构同 /v1/responses。',
  ],
  [
    'result.choices',
    'array<object>',
    'Chat 文本完成时',
    '—',
    '模型输出，结构同 /v1/chat/completions。',
  ],
  ['failureReason', 'string | null', '是', 'null（未失败）', '失败或取消原因。'],
  ['queuedAt', 'string', '是', '—', '入队时间，RFC 3339。'],
  ['startedAt', 'string | null', '是', 'null（未开始）', '开始处理时间，RFC 3339。'],
  ['updatedAt', 'string', '是', '—', '最近状态更新时间，RFC 3339。'],
  ['completedAt', 'string | null', '是', 'null（未结束）', '完成时间，RFC 3339。'],
]).concat(
  prefix(
    imageResponse.map((f) => ({ ...f, required: '图片 completed 时返回' })),
    'result.'
  )
)
export const fileFields = output([
  ['id', 'string', '是', '—', '文件 ID，可作为 file_id 引用。'],
  ['object', 'string', '是', 'file', '文件对象类型。'],
  ['bytes', 'integer', '是', '—', '文件字节数。'],
  ['created_at', 'integer', '是', '—', '上传时间，Unix 秒。'],
  ['filename', 'string', '是', '—', '去除目录部分后的文件名。'],
  ['purpose', 'string', '是', '—', '上传时指定的用途。'],
])
export const videoFields = output([
  ['id', 'string', '是', '—', '视频任务 ID。'],
  ['object', 'string', '是', 'video', '视频对象类型。'],
  ['model', 'string', '是', '—', '视频模型。'],
  ['status', 'string', '是', '—', 'queued / in_progress / completed / failed。'],
  ['progress', 'integer', '是', '—', '进度百分比，0–100。'],
  ['created_at', 'integer', '是', '—', '创建时间，Unix 秒。'],
  ['completed_at', 'integer | null', '是', 'null（未完成）', '完成时间，Unix 秒。'],
  ['seconds', 'string', '是', '—', '生成时长，数字字符串。'],
  ['size', 'string', '是', '—', '视频尺寸，宽x高。'],
  ['quality', 'string', '是', '—', '质量等级。'],
  ['prompt', 'string', '是', '—', '创建视频时的提示词。'],
  ['expires_at', 'integer | null', '是', 'null', '当前未设置自动过期时间。'],
  ['remixed_from_video_id', 'string | null', '是', 'null', '当前未实现视频混编来源。'],
  ['error', 'object | null', '是', 'null（未失败）', '失败信息。'],
  ['error.code', 'string', '失败时返回', '—', '机器可读错误码。'],
  ['error.message', 'string', '失败时返回', '—', '失败说明。'],
])
