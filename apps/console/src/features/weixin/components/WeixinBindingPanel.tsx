import { MessageCircle, RefreshCw, Send, Trash2 } from 'lucide-react'
import { KtButton } from '@/shared/ui'
import type { WeixinSettingsView } from '../types/weixin'
import { formatDateTime, StatusPill, WeixinPanel } from './WeixinShared'

interface WeixinBindingPanelProps {
  settings: WeixinSettingsView
  acting: boolean
  onStartBinding: () => void
  onSendTest: () => void
  onRemoveBinding: () => void
}

export function WeixinBindingPanel({
  settings,
  acting,
  onStartBinding,
  onSendTest,
  onRemoveBinding,
}: WeixinBindingPanelProps) {
  const active = settings.status === 'active'
  const bound = settings.status !== 'unbound'
  return (
    <WeixinPanel title="微信绑定">
      <BindingHeader settings={settings} />
      <BindingInfoGrid settings={settings} />
      <BindingActions
        acting={acting}
        active={active}
        bound={bound}
        onStartBinding={onStartBinding}
        onSendTest={onSendTest}
        onRemoveBinding={onRemoveBinding}
      />
    </WeixinPanel>
  )
}

function BindingHeader({ settings }: { settings: WeixinSettingsView }) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
      <div
        style={{
          width: 40,
          height: 40,
          borderRadius: 8,
          display: 'grid',
          placeItems: 'center',
          background: 'var(--color-brand-bg)',
          color: 'var(--color-brand)',
        }}
      >
        <MessageCircle size={20} />
      </div>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
        <span style={{ color: 'var(--text-bright)', fontWeight: 600 }}>
          {settings.maskedUserId || '尚未绑定微信'}
        </span>
        <StatusPill status={settings.status} />
      </div>
    </div>
  )
}

function BindingInfoGrid({ settings }: { settings: WeixinSettingsView }) {
  return (
    <div
      style={{
        display: 'grid',
        gridTemplateColumns: '1fr 1fr',
        gap: 10,
        color: 'var(--text-secondary)',
        fontSize: 13,
      }}
    >
      <InfoItem label="最近收到" value={formatDateTime(settings.lastInboundAt)} />
      <InfoItem label="最近发送" value={formatDateTime(settings.lastOutboundAt)} />
    </div>
  )
}

function BindingActions({
  acting,
  active,
  bound,
  onStartBinding,
  onSendTest,
  onRemoveBinding,
}: Omit<WeixinBindingPanelProps, 'settings'> & { active: boolean; bound: boolean }) {
  return (
    <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
      <KtButton variant="primary" loading={acting} onClick={onStartBinding}>
        <RefreshCw size={14} />
        {bound ? '重新绑定' : '扫码绑定'}
      </KtButton>
      <KtButton disabled={!active || acting} onClick={onSendTest}>
        <Send size={14} />
        发送测试
      </KtButton>
      <KtButton variant="danger" disabled={!bound || acting} onClick={onRemoveBinding}>
        <Trash2 size={14} />
        移除绑定
      </KtButton>
    </div>
  )
}

function InfoItem({ label, value }: { label: string; value: string }) {
  return (
    <div
      style={{
        minWidth: 0,
        padding: 10,
        borderRadius: 8,
        background: 'rgba(255,255,255,0.03)',
      }}
    >
      <div style={{ color: 'var(--text-tertiary)', fontSize: 12 }}>{label}</div>
      <div style={{ marginTop: 4, color: 'var(--text-primary)', overflowWrap: 'anywhere' }}>
        {value}
      </div>
    </div>
  )
}
