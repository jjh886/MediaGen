import {
  useRef,
  useState,
  useEffect,
  useCallback,
  type KeyboardEvent,
  type MutableRefObject,
} from 'react'

type TabItem = { key: string; label: string }

interface KtTabsProps {
  items: TabItem[]
  activeKey: string
  onChange: (key: string) => void
}

interface UnderlineStyle {
  left: number
  width: number
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

function useTabUnderline(activeKey: string) {
  const containerRef = useRef<HTMLDivElement>(null)
  const tabRefs = useRef<Map<string, HTMLButtonElement>>(new Map())
  const [underlineStyle, setUnderlineStyle] = useState<UnderlineStyle>({
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

  return { containerRef, tabRefs, underlineStyle }
}

function useTabKeyboard(
  items: TabItem[],
  tabRefs: MutableRefObject<Map<string, HTMLButtonElement>>,
  onChange: (key: string) => void
) {
  const focusTab = useCallback(
    (index: number) => {
      const next = items[index]
      if (!next) return
      onChange(next.key)
      tabRefs.current.get(next.key)?.focus()
    },
    [items, onChange, tabRefs]
  )

  return useCallback(
    (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
      const next = nextTabIndex(event.key, index, items.length)
      if (next === null) return
      event.preventDefault()
      focusTab(next)
    },
    [focusTab, items.length]
  )
}

function nextTabIndex(key: string, index: number, length: number): number | null {
  if (length === 0) return null
  if (key === 'ArrowRight') return (index + 1) % length
  if (key === 'ArrowLeft') return (index - 1 + length) % length
  return null
}

function setTabRef(
  refs: MutableRefObject<Map<string, HTMLButtonElement>>,
  key: string,
  element: HTMLButtonElement | null
) {
  if (element) refs.current.set(key, element)
  else refs.current.delete(key)
}

function TabButton({
  active,
  item,
  onClick,
  onKeyDown,
  setRef,
}: {
  active: boolean
  item: TabItem
  onClick: () => void
  onKeyDown: (event: KeyboardEvent<HTMLButtonElement>) => void
  setRef: (element: HTMLButtonElement | null) => void
}) {
  return (
    <button
      ref={setRef}
      role="tab"
      aria-selected={active}
      tabIndex={active ? 0 : -1}
      style={{
        ...styles.tab,
        color: active ? '#5AB0FE' : 'rgba(255,255,255,0.6)',
      }}
      onClick={onClick}
      onKeyDown={onKeyDown}
    >
      {item.label}
    </button>
  )
}

function TabUnderline({ style }: { style: UnderlineStyle }) {
  return <div style={{ ...styles.underline, left: style.left, width: style.width }} />
}

function KtTabs({ items, activeKey, onChange }: KtTabsProps) {
  const { containerRef, tabRefs, underlineStyle } = useTabUnderline(activeKey)
  const handleKeyDown = useTabKeyboard(items, tabRefs, onChange)

  return (
    <div ref={containerRef} role="tablist" style={styles.container}>
      {items.map((item, index) => (
        <TabButton
          key={item.key}
          active={item.key === activeKey}
          item={item}
          setRef={(element) => setTabRef(tabRefs, item.key, element)}
          onClick={() => onChange(item.key)}
          onKeyDown={(event) => handleKeyDown(event, index)}
        />
      ))}
      <TabUnderline style={underlineStyle} />
    </div>
  )
}

export type { KtTabsProps }
export { KtTabs }
