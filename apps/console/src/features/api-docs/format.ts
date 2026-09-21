import { guideMarkdown } from './guide'
import { documents, type ApiDocument } from './catalog'
import {
  locations,
  errorFields,
  queueErrorFields,
  type ApiField,
  type FieldLocation,
} from './schema'

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
  ['403', 'permission_error / account_permission_denied', '无接口或资源权限；检查账号及授权范围。'],
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
    '429',
    'insufficient_quota',
    '当前模型的上游额度为 0；检查 API 额度或订阅。排队不会增加上游额度，此类任务会直接失败。',
  ],
  [
    '500 / 502 / 503',
    'server_error / realtime_upstream_rejected / QUEUE_DISABLED / RESULT_STORAGE_ERROR',
    '上游、连接或存储暂时不可用；通过 x-request-id 排查。提交状态不明时不要盲目重放生成请求。',
  ],
]
export const guide =
  guideMarkdown() +
  '\n## 常见错误码\n\n' +
  errors.map(([status, code, help]) => `- HTTP ${status} · ${code}：${help}`).join('\n') +
  '\n'

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
function tableCell(value: string): string {
  return value.replaceAll('|', '\\|').replaceAll('\n', '<br>')
}
export function parameterTable(items: ApiField[], response = false): string {
  const headings = [
    response ? '字段名' : '参数名',
    '类型',
    response ? '是否必返 / 条件' : '是否必填',
    response ? '固定值 / 空值' : '默认值',
    '说明',
  ]
  return [
    '| ' + headings.join(' | ') + ' |',
    '| --- | --- | --- | --- | --- |',
    ...items.map(
      (field) =>
        '| ' +
        [field.name, field.type, field.required, field.defaultValue, field.description]
          .map(tableCell)
          .join(' | ') +
        ' |'
    ),
  ].join('\n')
}
export function fullDocumentation(): string {
  const commonErrors =
    '\n## 错误响应字段\n\n' +
    parameterTable(errorFields, true) +
    '\n\n### 排队接口错误字段\n\n' +
    parameterTable(queueErrorFields, true)
  return (
    guide +
    commonErrors +
    '\n\n' +
    documents
      .map((doc) => {
        const requestTables = (['header', 'path', 'query', 'body', 'event'] as FieldLocation[])
          .flatMap((location) => {
            const items = doc.requestFields.filter((field) => field.location === location)
            return items.length ? [`#### ${locations[location]}\n\n${parameterTable(items)}`] : []
          })
          .join('\n\n')
        const stream = doc.streamFields
          ? '\n\n#### 流式响应字段（stream=true）\n\n' + parameterTable(doc.streamFields, true)
          : ''
        return `## ${doc.method} ${doc.path} · ${doc.title}

${doc.description}

### 请求参数

${requestTables}

### 响应参数

“—”表示没有统一固定值；条件字段仅在指定场景返回；[] 表示数组项。

${parameterTable(doc.responseFields, true)}${stream}

### 请求示例

\`\`\`${doc.method === 'WS' ? 'javascript' : 'bash'}
${requestExample(doc)}
\`\`\`

### 响应示例（HTTP ${doc.status || (doc.method === 'WS' ? 101 : 200)}）

\`\`\`
${typeof doc.response === 'string' ? doc.response : JSON.stringify(doc.response, null, 2)}
\`\`\`

### 补充说明

${doc.notes.map((note) => '- ' + note).join('\n')}
`
      })
      .join('\n')
  )
}
