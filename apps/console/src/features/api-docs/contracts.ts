import type { ApiExample } from './samples'
import {
  fields,
  output,
  prefix,
  pageFields,
  listFields,
  errorFields,
  type ApiContract,
  type ApiField,
} from './schema'
import { modelFields } from './modelFields'
import { responseRequest, chatRequest, compactRequest } from './textRequestFields'
import {
  responseFields,
  responseStreamFields,
  chatResponseFields,
  chatStreamFields,
} from './textResponseFields'
import {
  imageRequest,
  speechRequest,
  transcriptionRequest,
  videoRequest,
} from './mediaRequestFields'
import {
  imageResponse,
  imageStream,
  queueAccepted,
  queueItem,
  taskFields,
  fileFields,
  videoFields,
} from './mediaResponseFields'
import { realtimeRequest, realtimeResponse, liveRequest, liveResponse } from './realtimeFields'

const contract = (
  requestFields: ApiField[],
  responseFields: ApiField[],
  streamFields?: ApiField[]
): ApiContract => ({ requestFields, responseFields, streamFields })
const binary = (mime: string, detail: string) =>
  output([
    ['Content-Type（响应头）', 'string', '是', mime, '用于识别响应体的媒体类型。'],
    ['（响应体）', 'binary', '是', '—', detail],
  ])
const deletion = (object: string) =>
  output([
    ['id', 'string', '是', '—', '被删除的资源 ID。'],
    ['object', 'string', '是', object, '删除结果对象类型。'],
    ['deleted', 'boolean', '是', '—', '是否删除成功。'],
  ])
const success = output([['success', 'boolean', '是', 'true', '排队接口成功标志。']])

function endpointContract(doc: ApiExample): ApiContract {
  const path = doc.path.split('?')[0]
  if (path === '/v1/models')
    return contract(
      fields(
        [
          ['refresh', 'boolean', '否', 'false', 'true 主动刷新账号模型目录。'],
          ['account_id', 'string', '否', '所有可访问账号', '仅查询当前 API Key 有权访问的账号。'],
        ],
        'query'
      ),
      output([
        ['object', 'string', '是', 'list', '模型列表。'],
        ['data', 'array<object>', '是', '—', '可路由模型数组；无模型时为空。'],
        [
          'refresh_errors',
          'array<object>',
          '是',
          '[]（无错误）',
          '模型发现未成功的账号，不包含认证凭据。',
        ],
        ['refresh_errors[].account_id', 'string', '错误条目返回', '—', '模型发现失败的账号。'],
        ['refresh_errors[].code', 'string', '错误条目返回', '—', '失败代码。'],
      ]).concat(prefix(modelFields, 'data[].'))
    )
  if (path === '/v1/models/{model}') return contract([], modelFields)
  if (path === '/v1/responses' && doc.method === 'WS')
    return contract(
      [
        ...fields([['type', 'string', '是', 'response.create', '客户端事件类型。']], 'event'),
        ...responseRequest
          .filter(
            (f) =>
              !['stream', 'background', 'stream_options', 'async', 'callback_url'].includes(f.name)
          )
          .map((f) => ({
            ...f,
            location: 'event' as const,
            defaultValue: f.name === 'store' ? 'false' : f.defaultValue,
            description:
              f.name === 'store'
                ? '默认仅在连接内保存上下文；true 持久保存供 HTTP 查询。'
                : f.description,
          })),
      ],
      responseStreamFields
    )
  if (path === '/v1/responses')
    return contract(
      responseRequest,
      doc.status === 202 ? queueAccepted : responseFields,
      doc.status === 202 ? undefined : responseStreamFields
    )
  if (path === '/v1/chat/completions')
    return contract(chatRequest, chatResponseFields, chatStreamFields)
  if (path === '/v1/responses/compact')
    return contract(
      compactRequest,
      output([
        ['id', 'string', '是', '—', '压缩结果 ID。'],
        ['object', 'string', '是', 'response.compaction', '压缩对象类型。'],
        ['output', 'array<object>', '是', '—', '压缩后的输入项，供后续 Responses 使用。'],
        ['output[].type', 'string', '是', '—', 'compaction 或保留的消息类型。'],
        [
          'output[].encrypted_content',
          'string',
          'compaction 项返回',
          '—',
          '上游不透明内容，原样保留，不可自行解码修改。',
        ],
        ['usage', 'object', '上游提供时', '—', 'token 用量。'],
      ])
    )
  if (path.endsWith('/input_items'))
    return contract(
      pageFields('20', 100),
      listFields(
        output([
          ['id', 'string', '是', '—', '输入项 ID。'],
          ['type', 'string', '条件返回', '—', 'message 或工具项类型。'],
          ['role', 'string', '消息项返回', '—', '原始消息角色。'],
          [
            'content',
            'string | array<object>',
            '消息项返回',
            '—',
            '原始输入内容，结构同 Responses input。',
          ],
        ])
      )
    )
  if (path === '/v1/responses/{response_id}')
    return contract([], doc.method === 'DELETE' ? deletion('response.deleted') : responseFields)
  if (path.startsWith('/v1/images/'))
    return contract(
      imageRequest(!path.endsWith('/generations'), path.endsWith('/variations')),
      doc.status === 202 ? queueAccepted : doc.status === 400 ? errorFields : imageResponse,
      doc.status ? undefined : imageStream
    )
  if (path === '/api/v2/generation-queue')
    return contract(
      fields(
        [
          ['limit', 'integer', '否', '40', '每页条数，取值 1–100。'],
          ['offset', 'integer', '否', '0', '跳过的记录数。'],
          [
            'taskKind',
            'string',
            '否',
            '全部类型',
            'image 查询图片队列，text 查询文本队列；两类队列顺序独立。',
          ],
        ],
        'query'
      ),
      [
        ...success,
        ...output([
          ['data.total', 'integer', '是', '—', '当前 API Key 的等待任务总数。'],
          ['data.items', 'array<object>', '是', '—', '当前页等待任务。'],
        ]),
        ...prefix(queueItem, 'data.items[].'),
      ]
    )
  if (path === '/api/v2/generation-tasks/{task_id}')
    return contract([], [...success, ...prefix(taskFields, 'data.')])
  if (path === '/api/v2/generation-tasks/batch')
    return contract(
      fields([
        [
          'taskIds',
          'array<string>',
          '是',
          '—',
          '任务 ID 数组，最多 1000 项；会去除空项、空白和重复 ID。',
        ],
      ]),
      [
        ...success,
        ...output([
          ['data.items', 'array<object>', '是', '—', '存在且属于当前 Key 的任务详情。'],
          [
            'data.notFoundTaskIds',
            'array<string>',
            '是',
            '[]（全部找到）',
            '不存在或无权访问的任务 ID。',
          ],
        ]),
        ...prefix(taskFields, 'data.items[].'),
      ]
    )
  if (path === '/api/v2/generation-queue/actions')
    return contract(
      fields([
        ['action', 'string', '是', '—', 'promote 提升等待优先级，cancel 取消任务。'],
        [
          'taskIds',
          'array<string>',
          '是',
          '—',
          '待操作的任务 ID 数组，仅限当前 API Key 所有的任务。',
        ],
      ]),
      [
        ...success,
        ...output([
          ['data.action', 'string', '是', '—', '执行的 promote 或 cancel 操作。'],
          ['data.affected', 'integer', '是', '—', '实际受影响的任务数。'],
        ]),
      ]
    )
  if (path === '/v1/audio/speech')
    return contract(
      speechRequest,
      binary(
        '按 response_format',
        '完整音频字节；不返回 JSON 或 URL。MP3 为 audio/mpeg，PCM 为裸音频。'
      )
    )
  if (path === '/v1/audio/transcriptions' || path === '/v1/audio/translations')
    return contract(
      transcriptionRequest,
      output([
        ['text', 'string', 'response_format=json 时', '—', '转写文本；translations 为英文译文。'],
        [
          '（响应体）',
          'string',
          'response_format=text 时',
          '—',
          'text/plain 纯文本，不能按 JSON 解析。',
        ],
      ])
    )
  if (path === '/v1/realtime') return contract(realtimeRequest, realtimeResponse)
  if (path === '/v1/live/sessions') return contract(liveRequest, liveResponse)
  if (path === '/v1/live')
    return contract(
      fields([
        ['session', 'object', '是', '—', '会话配置，必须包含 model。'],
        ['session.model', 'string', '是', '—', 'Live 模型 ID。'],
        [
          'sdp',
          'string',
          '是',
          '—',
          '完整 SDP offer，以 v=0 开头；此兼容路径不用 transport 包装。',
        ],
      ]),
      output([
        ['Location（响应头）', 'string', '是', '—', '/v1/live/{call_id}，用于建立辅助事件连接。'],
        ['Content-Type（响应头）', 'string', '是', 'application/sdp', '响应体是 SDP 文本。'],
        ['（响应体）', 'string', '是', '—', 'SDP answer；不是 JSON。'],
      ])
    )
  if (path.startsWith('/v1/live/') && doc.method === 'WS')
    return contract(
      realtimeRequest.filter((f) => f.location === 'event'),
      realtimeResponse
    )
  if (path.endsWith('/hangup'))
    return contract(
      [],
      output([['（响应体）', 'empty', '无响应字段', '空', 'HTTP 200 表示挂断请求已提交。']])
    )
  if (path === '/v1/files')
    return doc.method === 'POST'
      ? contract(
          fields([
            ['file', 'file', '是', '—', 'multipart 文件，上传内容最多 50 MiB。'],
            [
              'purpose',
              'string',
              '否',
              'assistants',
              '文件用途，例如 assistants、vision；允许值以服务端支持为准。',
            ],
          ]),
          fileFields
        )
      : contract(
          [
            ...pageFields('10000', 10000),
            ...fields([['purpose', 'string', '否', '不筛选', '只返回指定用途的文件。']], 'query'),
          ],
          listFields(fileFields)
        )
  if (path === '/v1/files/{file_id}/content')
    return contract([], binary('application/octet-stream', '上传时的原始文件内容。'))
  if (path === '/v1/files/{file_id}')
    return contract([], doc.method === 'DELETE' ? deletion('file') : fileFields)
  if (path === '/v1/videos')
    return doc.method === 'POST'
      ? contract(videoRequest, videoFields)
      : contract(
          pageFields('20', 100),
          listFields(videoFields).filter((f) => !['first_id', 'last_id'].includes(f.name))
        )
  if (path === '/v1/videos/{video_id}/content')
    return contract(
      fields(
        [
          [
            'variant',
            'string',
            '否',
            'video',
            'video 下载视频；thumbnail 下载缩略图（上游提供时）。',
          ],
        ],
        'query'
      ),
      binary(
        '上游媒体类型',
        '视频通常为 video/mp4；缩略图通常为 image/jpeg，具体以 Content-Type 为准。'
      )
    )
  if (path === '/v1/videos/{video_id}' || path.endsWith('/cancel'))
    return contract([], doc.method === 'DELETE' ? deletion('video.deleted') : videoFields)
  throw new Error(`未定义接口字段表：${doc.method} ${doc.path}`)
}
export function describeContract(doc: ApiExample): ApiContract {
  const schema = endpointContract(doc)
  const headers = fields(
    [
      [
        'Authorization',
        'string',
        '是',
        '—',
        'Bearer API_KEY；在 API Key 页面创建，不能使用平台登录令牌代替。',
      ],
      [
        'X-Client-Request-Id',
        'string',
        '否',
        '未设置',
        '1–512 个可打印 ASCII 字符，用于调用方追踪。',
      ],
    ],
    'header'
  )
  if (doc.method === 'POST')
    headers.push(
      ...fields(
        [
          [
            'Content-Type',
            'string',
            '是（有请求体时）',
            '—',
            doc.form
              ? 'multipart/form-data；curl -F 自动生成包含 boundary 的请求头，不要手动省略 boundary。'
              : 'application/json；图片编辑和视频创建也支持 multipart/form-data。',
          ],
        ],
        'header'
      )
    )
  const path = [...doc.path.matchAll(/\{([^}]+)\}/g)].map((match) => match[1])
  return {
    ...schema,
    requestFields: [
      ...headers,
      ...fields(
        path.map((name) => [
          name,
          'string',
          '是',
          '—',
          '替换为实际资源 ID；仅可访问当前 API Key 所属资源。',
        ]),
        'path'
      ),
      ...schema.requestFields,
    ],
  }
}
