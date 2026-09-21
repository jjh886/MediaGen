import { useState } from 'react'
import { KtButton } from '@/shared/ui'
import type { ApiDocument } from './catalog'
import { requestExample } from './format'
import { locations, type FieldLocation } from './schema'
import ApiFieldTable from './ApiFieldTable'

export default function ApiEndpoint({
  doc,
  index,
  forceOpen,
  onCopy,
}: {
  doc: ApiDocument
  index: number
  forceOpen: boolean
  onCopy: (value: string) => Promise<void>
}) {
  const [expanded, setExpanded] = useState(false)
  const open = forceOpen || expanded
  return (
    <details
      className="api-docs__endpoint"
      open={open}
      onToggle={(event) => setExpanded(event.currentTarget.open)}
    >
      <summary>
        <span className={`api-docs__method api-docs__method--${doc.method.toLowerCase()}`}>
          {doc.method}
        </span>
        <code>{doc.path}</code>
        <span>{doc.title}</span>
      </summary>
      {open && (
        <div className="api-docs__body">
          <p>{doc.description}</p>
          <div className="api-docs__section-heading">
            <span>01</span>
            <h2>请求参数</h2>
          </div>
          {(['header', 'path', 'query', 'body', 'event'] as FieldLocation[]).map((location) => {
            const items = doc.requestFields.filter((field) => field.location === location)
            return (
              items.length > 0 && (
                <ApiFieldTable
                  key={location}
                  title={
                    location === 'body' && doc.form
                      ? '请求体参数 · multipart/form-data'
                      : locations[location]
                  }
                  fields={items}
                />
              )
            )
          })}
          {!doc.requestFields.some(
            (field) => field.location === 'body' || field.location === 'event'
          ) && <p className="api-docs__field-note">此接口无需提交请求体。</p>}
          <div className="api-docs__section-heading">
            <span>02</span>
            <h2>响应参数</h2>
          </div>
          <p className="api-docs__field-note">
            “—”表示没有统一固定值；条件字段仅在说明的场景出现。数组项使用 [] 表示。
          </p>
          <ApiFieldTable
            title={doc.method === 'WS' ? '服务端事件字段' : `响应字段 · HTTP ${doc.status || 200}`}
            fields={doc.responseFields}
            response
          />
          {doc.streamFields && (
            <ApiFieldTable title="流式响应字段 · stream=true" fields={doc.streamFields} response />
          )}
          <div className="api-docs__section-heading">
            <span>03</span>
            <h2>调用示例</h2>
          </div>
          <div className="api-docs__examples">
            <section>
              <div className="api-docs__code-title">
                请求示例
                <KtButton
                  size="sm"
                  aria-label={`复制请求示例 ${index + 1}`}
                  onClick={() => void onCopy(requestExample(doc))}
                >
                  复制
                </KtButton>
              </div>
              <pre>{requestExample(doc)}</pre>
            </section>
            <section>
              <div className="api-docs__code-title">
                响应示例 · {doc.status || (doc.method === 'WS' ? '101 WebSocket' : '200 OK')}
              </div>
              <pre>
                {typeof doc.response === 'string'
                  ? doc.response
                  : JSON.stringify(doc.response, null, 2)}
              </pre>
            </section>
          </div>
          {doc.notes.length > 0 && (
            <details className="api-docs__notes">
              <summary>补充说明与能力限制</summary>
              <ul>
                {doc.notes.map((note) => (
                  <li key={note}>{note}</li>
                ))}
              </ul>
            </details>
          )}
        </div>
      )}
    </details>
  )
}
