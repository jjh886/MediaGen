import React, { useState, useRef, useEffect } from 'react'

interface KtSelectProps {
  value: string
  onChange: (value: string) => void
  options: { value: string; label: string }[]
  placeholder?: string
  className?: string
}

function useCloseOnOutside(
  open: boolean,
  containerRef: React.RefObject<HTMLDivElement | null>,
  close: () => void
) {
  useEffect(() => {
    if (!open) return

    const handleClickOutside = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        close()
      }
    }

    document.addEventListener('mousedown', handleClickOutside)
    return () => document.removeEventListener('mousedown', handleClickOutside)
  }, [close, containerRef, open])
}

function triggerStyle(open: boolean, hasValue: boolean): React.CSSProperties {
  return {
    height: 36,
    backgroundColor: '#1E1F21',
    border: `1px solid ${open ? '#5AB0FE' : 'rgba(255,255,255,0.05)'}`,
    borderRadius: 8,
    color: hasValue ? 'rgba(255,255,255,0.8)' : 'rgba(255,255,255,0.3)',
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
}

function dropdownStyle(): React.CSSProperties {
  return {
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
}

function SelectTrigger({
  open,
  label,
  onClick,
}: {
  open: boolean
  label: string
  onClick: () => void
}) {
  return (
    <div style={triggerStyle(open, Boolean(label))} onClick={onClick}>
      <span>{label}</span>
      <span style={{ fontSize: 12, marginLeft: 8, color: 'rgba(255,255,255,0.3)' }}>▾</span>
    </div>
  )
}

function SelectDropdown({
  value,
  options,
  onSelect,
}: {
  value: string
  options: KtSelectProps['options']
  onSelect: (value: string) => void
}) {
  return (
    <div style={dropdownStyle()}>
      {options.map((option) => (
        <OptionItem
          key={option.value}
          label={option.label}
          selected={option.value === value}
          onClick={() => onSelect(option.value)}
        />
      ))}
    </div>
  )
}

function KtSelect({ value, onChange, options, placeholder, className }: KtSelectProps) {
  const [open, setOpen] = useState(false)
  const containerRef = useRef<HTMLDivElement>(null)
  const selectedLabel = options.find((o) => o.value === value)?.label ?? placeholder ?? ''

  const close = () => setOpen(false)
  useCloseOnOutside(open, containerRef, close)

  const handleSelect = (nextValue: string) => {
    onChange(nextValue)
    setOpen(false)
  }

  return (
    <div ref={containerRef} className={className} style={{ position: 'relative' }}>
      <SelectTrigger open={open} label={selectedLabel} onClick={() => setOpen((prev) => !prev)} />
      {open && <SelectDropdown value={value} options={options} onSelect={handleSelect} />}
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
