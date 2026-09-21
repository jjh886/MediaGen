import { useState } from 'react'
import { BookOpen, Copy, Search } from 'lucide-react'
import { KtButton, KtInput } from '@/shared/ui'
import { documents } from './catalog'
import { errors, fullDocumentation } from './format'
import ApiGuide from './ApiGuide'
import ApiEndpoint from './ApiEndpoint'
import ApiFieldTable from './ApiFieldTable'
import { errorFields, queueErrorFields } from './schema'
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
      `${doc.path} ${doc.title} ${doc.description} ${doc.requestFields.map((f) => f.name).join(' ')} ${doc.responseFields.map((f) => f.name).join(' ')}`
        .toLowerCase()
        .includes(filter.toLowerCase())
  )
  return (
    <div className="api-docs">
      <header className="api-docs__header">
        <div>
          <div className="api-docs__eyebrow">
            <BookOpen size={15} /> DEVELOPER REFERENCE
          </div>
          <h1>接口文档</h1>
          <p>逐项查看请求参数、默认值、响应字段与调用示例。</p>
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
      <ApiGuide />
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
        <ApiEndpoint
          key={`${doc.method}-${doc.path}-${doc.title}`}
          doc={doc}
          index={index}
          forceOpen={Boolean(filter)}
          onCopy={copy}
        />
      ))}
      {!visible.length && <p>没有匹配的接口，请换一个关键词。</p>}
      <section className="api-docs__errors">
        <h2>错误响应</h2>
        <ApiFieldTable title="OpenAI 兼容接口错误字段" fields={errorFields} response />
        <ApiFieldTable title="排队接口错误字段" fields={queueErrorFields} response />
        <h3>常见错误码</h3>
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
