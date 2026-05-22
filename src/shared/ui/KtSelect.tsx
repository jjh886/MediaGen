import React, { useState, useRef, useEffect } from 'react'

interface KtSelectProps {
  value: string
  onChange: (value: string) => void
  options: { value: string; label: string }[]
  placeholder?: string
  className?: string
}

function KtSelect({ value, onChange, options, placeholder, className }: KtSelectProps) {
  const [open, setOpen] = useState(false)
  const containerRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) return

    const handleClickOutside = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpen(false)
      }
    }

    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [open])

  const selectedLabel = options.find((o) => o.value === value)?.label

  const triggerStyle: React.CSSProperties = {
    height: 36,
    backgroundColor: '#1E1F21',
    border: `1px solid ${open ? '#5AB0FE' : 'rgba(255,255,255,0.05)'}`,
    borderRadius: 8,
    color: selectedLabel ? 'rgba(255,255,255,0.8)' : 'rgba(255,255,255,0.3)',
    padding: '0 12px',
    fontSize: 14,
    outline: 'none',
    transition: 'border-color 200ms ease',
    boxSizing: 'border-box',
    width: '100%',
    cursor: 'pointer',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    userSelect: 'none',
  }

  const dropdownStyle: React.CSSProperties = {
    position: 'absolute',
    top: 'calc(100% + 4px)',
    left: 0,
    right: 0,
    backgroundColor: '#1E1F21',
    borderRadius: 12,
    boxShadow: '0 4px 24px rgba(0,0,0,0.5)',
    padding: '4px 0',
    zIndex: 1000,
    overflow: 'auto',
  }

  return (
    <div ref={containerRef} className={className} style={{ position: 'relative' }}>
      <div style={triggerStyle} onClick={() => setOpen((prev) => !prev)}>
        <span>{selectedLabel ?? placeholder ?? ''}</span>
        <span style={{ fontSize: 12, marginLeft: 8, color: 'rgba(255,255,255,0.3)' }}>▾</span>
      </div>

      {open && (
        <div style={dropdownStyle}>
          {options.map((option) => {
            const isSelected = option.value === value
            return (
              <OptionItem
                key={option.value}
                label={option.label}
                selected={isSelected}
                onClick={() => {
                  onChange(option.value)
                  setOpen(false)
                }}
              />
            )
          })}
        </div>
      )}
    </div>
  )
}

function OptionItem({
  label,
  selected,
  onClick,
}: {
  label: string
  selected: boolean
  onClick: () => void
}) {
  const [hovered, setHovered] = useState(false)

  const style: React.CSSProperties = {
    height: 36,
    display: 'flex',
    alignItems: 'center',
    padding: '0 12px',
    fontSize: 14,
    cursor: 'pointer',
    color: selected ? '#5AB0FE' : 'rgba(255,255,255,0.8)',
    backgroundColor: selected
      ? 'rgba(90,176,254,0.1)'
      : hovered
        ? 'rgba(255,255,255,0.05)'
        : 'transparent',
    transition: 'background-color 150ms ease',
  }

  return (
    <div
      style={style}
      onClick={onClick}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      {label}
    </div>
  )
}

KtSelect.displayName = 'KtSelect'

export { KtSelect }
export type { KtSelectProps }
