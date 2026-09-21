import type { ApiExample } from './samples'

export type FieldLocation = 'header' | 'path' | 'query' | 'body' | 'event' | 'response'
export interface ApiField {
  name: string
  type: string
  required: string
  defaultValue: string
  description: string
  location: FieldLocation
}
export type FieldRow = [
  name: string,
  type: string,
  required: string,
  defaultValue: string,
  description: string,
]
export function fields(rows: FieldRow[], location: FieldLocation = 'body'): ApiField[] {
  return rows.map(([name, type, required, defaultValue, description]) => ({
    name,
    type,
    required,
    defaultValue,
    description,
    location,
  }))
}
export const output = (rows: FieldRow[]) => fields(rows, 'response')
export const prefix = (items: ApiField[], parent: string) =>
  items.map((item) => ({ ...item, name: parent + item.name }))
export interface ApiContract {
  request: ApiField[]
  response: ApiField[]
  stream?: ApiField[]
}
export interface ApiDocument extends ApiExample, ApiContract {}
export const locations: Record<FieldLocation, string> = {
  header: '请求头',
  path: '路径参数',
  query: '查询参数',
  body: '请求体参数',
  event: '客户端事件参数',
  response: '响应字段',
}
export const errorFields = output([
  ['error', 'object', '是', '—', '请求失败时的错误对象；不会同时返回成功结果。'],
  ['error.message', 'string', '是', '—', '可读的错误说明。'],
  ['error.type', 'string', '是', '—', '错误类别，例如 invalid_request_error。'],
  [
    'error.param',
    'string | null',
    '条件返回',
    'null',
    '相关参数名；不能定位具体参数时为 null，部分接口省略。',
  ],
  ['error.code', 'string', '是', '—', '机器可读的错误码，参见常见错误码表。'],
])
export const queueErrorFields = output([
  ['success', 'boolean', '是', 'false', '排队接口失败标志。'],
  ['error.code', 'string', '是', '—', '大写业务错误码，例如 TASK_NOT_FOUND。'],
  ['error.detail', 'string', '是', '—', '失败原因说明。'],
])
export function pageFields(limit: string, max: number): ApiField[] {
  return fields(
    [
      ['limit', 'integer', '否', limit, `每页条数，取值 1–${max}。`],
      ['after', 'string', '否', '不使用游标', '从此资源 ID 之后读取；使用上一页的 last_id。'],
      ['order', 'string', '否', 'desc', 'asc 为正序，desc 为倒序。'],
    ],
    'query'
  )
}
export function listFields(items: ApiField[]): ApiField[] {
  return output([
    ['object', 'string', '是', 'list', '列表对象类型。'],
    ['data', 'array<object>', '是', '—', '当前页记录；无记录时为空数组。'],
    ['first_id', 'string | null', '是', '—', '当前页第一条记录 ID；空页为 null。'],
    ['last_id', 'string | null', '是', '—', '当前页最后一条记录 ID；空页为 null。'],
    ['has_more', 'boolean', '是', '—', '是否还有下一页。'],
  ]).concat(prefix(items, 'data[].'))
}
