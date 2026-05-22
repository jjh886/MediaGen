interface LogDetailRowProps {
  detail?: string
}

export function LogDetailRow({ detail }: LogDetailRowProps) {
  let formatted = detail ?? ''
  try {
    formatted = JSON.stringify(JSON.parse(formatted), null, 2)
  } catch {
    // detail 不是合法 JSON，保持原文
  }

  return (
    <tr>
      <td colSpan={4} style={{ padding: 0 }}>
        <div
          style={{
            background: '#252528',
            margin: '0 16px 8px',
            padding: 16,
            borderRadius: 8,
            maxHeight: 300,
            overflow: 'auto',
          }}
        >
          <pre
            style={{
              margin: 0,
              fontFamily: 'ui-monospace, "SF Mono", Menlo, monospace',
              fontSize: 13,
              lineHeight: 1.6,
              color: 'rgba(255,255,255,0.75)',
              whiteSpace: 'pre-wrap',
              wordBreak: 'break-all',
            }}
          >
            {formatted || '无详细信息'}
          </pre>
        </div>
      </td>
    </tr>
  )
}
