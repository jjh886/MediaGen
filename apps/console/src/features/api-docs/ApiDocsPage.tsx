import { useState } from 'react'
import { BookOpen, Copy, Search } from 'lucide-react'
import { KtButton, KtInput } from '@/shared/ui'
import { documents } from './catalog'
import { errors, fullDocumentation, guide, requestExample } from './format'
import './api-docs.css'

export default function ApiDocsPage() {
  const [filter, setFilter] = useState('')
  const [group, setGroup] = useState('全部')
  const [message, setMessage] = useState('')
  const copy = async (value: string) => {
    try {
      await navigator.clipboard.writeText(value)
      setMessage('已复制到剪贴板')
    } catch {
      setMessage('复制失败，请检查浏览器剪贴板权限。')
    }
  }
  const visible = documents.filter(
    (doc) =>
      (group === '全部' || doc.group === group) &&
      `${doc.path} ${doc.title} ${doc.description}`.toLowerCase().includes(filter.toLowerCase())
  )
  return (
    <div className="api-docs">
      <header className="api-docs__header">
        <div>
          <div className="api-docs__eyebrow">
            <BookOpen size={15} /> DEVELOPER REFERENCE
          </div>
          <h1>接口文档</h1>
          <p>从模型发现到生成结果，完整调用示例集中在这里。</p>
        </div>
        <KtButton variant="primary" onClick={() => void copy(fullDocumentation())}>
          <Copy size={16} />
          复制全部接口文档
        </KtButton>
      </header>
      {message && (
        <div role="status" className="api-docs__notice">
          {message}
        </div>
      )}
      <section className="api-docs__intro">
        <div>
          <span>BASE URL</span>
          <code>http://127.0.0.1:18080</code>
        </div>
        <div>
          <span>AUTHENTICATION</span>
          <code>Authorization: Bearer API_KEY</code>
        </div>
        <p>
          先查询模型能力，再选择接口。图片返回 Base64，音频返回二进制。排队、Live 等扩展会明确标注。
        </p>
      </section>
      <details className="api-docs__guide">
        <summary>调用约定、排队流程、流式输出和实时语音</summary>
        <pre>{guide}</pre>
      </details>
      <div className="api-docs__toolbar">
        <div className="api-docs__search">
          <Search size={16} />
          <KtInput
            aria-label="搜索接口"
            value={filter}
            onChange={(event) => setFilter(event.target.value)}
            placeholder="搜索接口路径或能力…"
          />
        </div>
        <div className="api-docs__groups">
          {['全部', ...new Set(documents.map((d) => d.group))].map((item) => (
            <button key={item} aria-pressed={group === item} onClick={() => setGroup(item)}>
              {item}
            </button>
          ))}
        </div>
      </div>
      <div className="api-docs__count">{visible.length} 个接口与调用场景</div>
      {visible.map((doc, index) => (
        <details
          key={`${doc.method}-${doc.path}-${doc.title}`}
          className="api-docs__endpoint"
          open={filter ? true : undefined}
        >
          <summary>
            <span className={`api-docs__method api-docs__method--${doc.method.toLowerCase()}`}>
              {doc.method}
            </span>
            <code>{doc.path}</code>
            <span>{doc.title}</span>
          </summary>
          <div className="api-docs__body">
            <p>{doc.description}</p>
            <ul>
              {doc.parameters.map((item) => (
                <li key={item}>{item}</li>
              ))}
            </ul>
            <div className="api-docs__examples">
              <section>
                <div className="api-docs__code-title">
                  请求示例
                  <KtButton
                    size="sm"
                    aria-label={`复制请求示例 ${index + 1}`}
                    onClick={() => void copy(requestExample(doc))}
                  >
                    复制
                  </KtButton>
                </div>
                <pre>{requestExample(doc)}</pre>
              </section>
              <section>
                <div className="api-docs__code-title">
                  返回示例 · {doc.status || (doc.method === 'WS' ? '101 WebSocket' : '200 OK')}
                </div>
                <pre>
                  {typeof doc.response === 'string'
                    ? doc.response
                    : JSON.stringify(doc.response, null, 2)}
                </pre>
              </section>
            </div>
          </div>
        </details>
      ))}
      {!visible.length && <p>没有匹配的接口，请换一个关键词。</p>}
      <section className="api-docs__errors">
        <h2>常见错误码</h2>
        <table>
          <thead>
            <tr>
              <th>HTTP</th>
              <th>错误码</th>
              <th>处理方式</th>
            </tr>
          </thead>
          <tbody>
            {errors.map(([status, code, help]) => (
              <tr key={status}>
                <td>{status}</td>
                <td>
                  <code>{code}</code>
                </td>
                <td>{help}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>
    </div>
  )
}
