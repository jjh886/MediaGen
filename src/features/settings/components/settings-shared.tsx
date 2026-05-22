import { KtInput } from '@/shared/ui'
import type { AppSettings } from '@/shared/services/settings-service'

export const cardStyle: React.CSSProperties = {
  background: '#1E1F21',
  borderRadius: 12,
  padding: 24,
  display: 'flex',
  flexDirection: 'column',
  gap: 16,
}

export const titleStyle: React.CSSProperties = {
  fontSize: 16,
  fontWeight: 600,
  color: 'rgba(255,255,255,0.9)',
  margin: 0,
}

export const labelStyle: React.CSSProperties = {
  fontSize: 12,
  color: 'rgba(255,255,255,0.5)',
  marginBottom: 4,
}

export const dotStyle = (on: boolean): React.CSSProperties => ({
  width: 8,
  height: 8,
  borderRadius: '50%',
  background: on ? '#52C41A' : '#FF4D4F',
  display: 'inline-block',
  marginRight: 6,
})

export function Field({
  label,
  hint,
  children,
}: {
  label: string
  hint?: string
  children: React.ReactNode
}) {
  return (
    <div>
      <div style={labelStyle}>
        {label}
        {hint && (
          <span style={{ marginLeft: 8, color: 'rgba(255,180,0,0.7)', fontSize: 11 }}>{hint}</span>
        )}
      </div>
      {children}
    </div>
  )
}

export function NumInput({ value, onChange }: { value: number; onChange: (v: number) => void }) {
  return (
    <KtInput
      type="number"
      value={String(value)}
      onChange={(e) => onChange(Number(e.target.value) || 0)}
    />
  )
}

/** 构造 string 类型 onChange handler */
export function strHandler(
  update: <K extends keyof AppSettings>(k: K, v: AppSettings[K]) => void,
  k: keyof AppSettings
) {
  return (e: React.ChangeEvent<HTMLInputElement>) =>
    update(k, e.target.value as AppSettings[typeof k])
}

/** 构造 typed setter */
export function setter(update: <K extends keyof AppSettings>(k: K, v: AppSettings[K]) => void) {
  return <K extends keyof AppSettings>(k: K) =>
    (v: AppSettings[K]) =>
      update(k, v)
}
