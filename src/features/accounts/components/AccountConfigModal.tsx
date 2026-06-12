import { useState, useEffect } from 'react'
import { KtModal, KtButton, KtInput, KtSelect } from '@/shared/ui'
import type { Account } from '../types/account'

interface AccountConfigModalProps {
  open: boolean
  onClose: () => void
  account: Account | null
  onSave: (
    id: string,
    updates: {
      tier: string
      concurrency_limit: number
      daily_limit: number
      total_limit: number
      task_interval_seconds: number
    }
  ) => Promise<void>
}

const tierOptions = [
  { value: 'free', label: 'Free' },
  { value: 'standard', label: 'Standard' },
  { value: 'pro', label: 'Pro' },
  { value: 'premium', label: 'Premium' },
]

const fieldStyle: React.CSSProperties = {
  display: 'flex',
  flexDirection: 'column',
  gap: 6,
}

const labelStyle: React.CSSProperties = {
  color: 'var(--text-secondary)',
  fontSize: 13,
}

function toNonNegativeInt(value: string) {
  const parsed = Number(value)
  return Number.isFinite(parsed) ? Math.max(0, Math.floor(parsed)) : 0
}

export default function AccountConfigModal({
  open,
  onClose,
  account,
  onSave,
}: AccountConfigModalProps) {
  const [tier, setTier] = useState('')
  const [concurrency, setConcurrency] = useState('')
  const [dailyLimit, setDailyLimit] = useState('')
  const [totalLimit, setTotalLimit] = useState('')
  const [taskInterval, setTaskInterval] = useState('')

  useEffect(() => {
    if (account) {
      setTier(account.tier)
      setConcurrency(String(account.concurrency_limit))
      setDailyLimit(String(account.daily_limit))
      setTotalLimit(String(account.total_limit))
      setTaskInterval(String(account.task_interval_seconds ?? 0))
    }
  }, [account])

  const handleSave = async () => {
    if (!account) return
    await onSave(account.id, {
      tier,
      concurrency_limit: toNonNegativeInt(concurrency),
      daily_limit: toNonNegativeInt(dailyLimit),
      total_limit: toNonNegativeInt(totalLimit),
      task_interval_seconds: toNonNegativeInt(taskInterval),
    })
    onClose()
  }

  return (
    <KtModal
      open={open}
      onClose={onClose}
      title="账号配置"
      closeOnOverlayClick={false}
      footer={
        <>
          <KtButton variant="default" onClick={onClose}>
            取消
          </KtButton>
          <KtButton variant="primary" onClick={handleSave}>
            保存
          </KtButton>
        </>
      }
    >
      <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
        <div style={fieldStyle}>
          <label style={labelStyle}>等级</label>
          <KtSelect value={tier} onChange={setTier} options={tierOptions} />
        </div>
        <div style={fieldStyle}>
          <label style={labelStyle}>并发数</label>
          <KtInput
            type="number"
            min={0}
            max={10}
            value={concurrency}
            onChange={(e) => setConcurrency(e.target.value)}
          />
        </div>
        <div style={fieldStyle}>
          <label style={labelStyle}>每日限额</label>
          <KtInput
            type="number"
            min={0}
            value={dailyLimit}
            onChange={(e) => setDailyLimit(e.target.value)}
          />
        </div>
        <div style={fieldStyle}>
          <label style={labelStyle}>总限额</label>
          <KtInput
            type="number"
            min={0}
            value={totalLimit}
            onChange={(e) => setTotalLimit(e.target.value)}
          />
        </div>
        <div style={fieldStyle}>
          <label style={labelStyle}>任务间隔（秒）</label>
          <KtInput
            type="number"
            min={0}
            value={taskInterval}
            onChange={(e) => setTaskInterval(e.target.value)}
          />
          <span style={{ color: 'var(--text-disabled)', fontSize: 12 }}>
            一个任务结束后，至少等待这么久再让这个账号接下一单；0 表示不等待。
          </span>
        </div>
      </div>
    </KtModal>
  )
}
