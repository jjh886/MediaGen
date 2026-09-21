import { useState } from 'react'
import { guideSections, type GuideBlock } from './guide'
import './api-guide.css'

function InlineText({ text }: { text: string }) {
  return (
    <>
      {text
        .split(/(`[^`]+`)/g)
        .map((part, i) => (part.startsWith('`') ? <code key={i}>{part.slice(1, -1)}</code> : part))}
    </>
  )
}
function GuideContent({ block }: { block: GuideBlock }) {
  switch (block.kind) {
    case 'paragraph':
      return (
        <p>
          <InlineText text={block.text} />
        </p>
      )
    case 'note':
      return (
        <aside className="api-guide__note">
          <strong>注意</strong>
          <p>
            <InlineText text={block.text} />
          </p>
        </aside>
      )
    case 'code':
      return (
        <figure className="api-guide__code">
          <figcaption>
            {block.title}
            <span>{block.language.toUpperCase()}</span>
          </figcaption>
          <pre>
            <code>{block.value}</code>
          </pre>
        </figure>
      )
    case 'steps':
      return (
        <ol className="api-guide__steps">
          {block.items.map((step, index) => (
            <li key={step.title}>
              <span className="api-guide__step-number" aria-hidden="true">
                {index + 1}
              </span>
              <div>
                <h4>{step.title}</h4>
                <p>
                  <InlineText text={step.text} />
                </p>
              </div>
            </li>
          ))}
        </ol>
      )
    case 'table':
      return (
        <div
          className="api-guide__table-scroll"
          role="region"
          aria-label={block.title}
          tabIndex={0}
        >
          <table>
            <caption>{block.title}</caption>
            <thead>
              <tr>
                {block.headings.map((heading) => (
                  <th key={heading} scope="col">
                    {heading}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {block.rows.map((row, index) => (
                <tr key={index}>
                  {row.map((cell, i) => (
                    <td key={i}>
                      <InlineText text={cell} />
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )
  }
}
export default function ApiGuide() {
  const [selected, setSelected] = useState('conventions')
  const section = guideSections.find((item) => item.id === selected)!
  return (
    <details className="api-docs__guide">
      <summary>调用约定、排队流程、流式输出和实时语音</summary>
      <div className="api-guide">
        <nav className="api-guide__nav" aria-label="调用指南章节">
          {guideSections.map((item) => (
            <button
              key={item.id}
              type="button"
              aria-pressed={item.id === selected}
              aria-controls="api-guide-content"
              onClick={() => setSelected(item.id)}
            >
              {item.title}
            </button>
          ))}
        </nav>
        <section
          id="api-guide-content"
          className="api-guide__content"
          aria-labelledby="api-guide-title"
        >
          <header>
            <h3 id="api-guide-title">{section.title}</h3>
            <p>{section.description}</p>
          </header>
          {section.blocks.map((block, i) => (
            <GuideContent key={`${section.id}-${i}`} block={block} />
          ))}
        </section>
      </div>
    </details>
  )
}
