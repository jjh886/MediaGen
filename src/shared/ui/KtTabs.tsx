import { useRef, useState, useEffect, useCallback } from 'react'

interface KtTabsProps {
  items: { key: string; label: string }[]
  activeKey: string
  onChange: (key: string) => void
}

const styles = {
  container: {
    display: 'flex',
    gap: 24,
    borderBottom: '1px solid rgba(255,255,255,0.05)',
    position: 'relative' as const,
  },
  tab: {
    background: 'none',
    border: 'none',
    outline: 'none',
    cursor: 'pointer',
    padding: '8px 0',
    fontSize: 14,
    lineHeight: '22px',
    transition: 'color 300ms ease',
    position: 'relative' as const,
    whiteSpace: 'nowrap' as const,
  },
  underline: {
    position: 'absolute' as const,
    bottom: 0,
    height: 2,
    background: '#5AB0FE',
    transition: 'left 300ms ease, width 300ms ease',
  },
} as const

function KtTabs({ items, activeKey, onChange }: KtTabsProps) {
  const containerRef = useRef<HTMLDivElement>(null)
  const tabRefs = useRef<Map<string, HTMLButtonElement>>(new Map())
  const [underlineStyle, setUnderlineStyle] = useState<{ left: number; width: number }>({
    left: 0,
    width: 0,
  })

  const updateUnderline = useCallback(() => {
    const el = tabRefs.current.get(activeKey)
    const container = containerRef.current
    if (el && container) {
      const containerRect = container.getBoundingClientRect()
      const tabRect = el.getBoundingClientRect()
      setUnderlineStyle({
        left: tabRect.left - containerRect.left,
        width: tabRect.width,
      })
    }
  }, [activeKey])

  useEffect(() => {
    updateUnderline()
  }, [updateUnderline])

  return (
    <div ref={containerRef} style={styles.container}>
      {items.map((item) => {
        const isActive = item.key === activeKey
        return (
          <button
            key={item.key}
            ref={(el) => {
              if (el) tabRefs.current.set(item.key, el)
              else tabRefs.current.delete(item.key)
            }}
            style={{
              ...styles.tab,
              color: isActive ? '#5AB0FE' : 'rgba(255,255,255,0.6)',
            }}
            onClick={() => onChange(item.key)}
          >
            {item.label}
          </button>
        )
      })}
      <div
        style={{
          ...styles.underline,
          left: underlineStyle.left,
          width: underlineStyle.width,
        }}
      />
    </div>
  )
}

export type { KtTabsProps }
export { KtTabs }
