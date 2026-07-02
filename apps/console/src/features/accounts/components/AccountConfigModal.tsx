import { useState, useEffect } from 'react'
import { KtModal, KtButton, KtInput, KtSelect } from '@/shared/ui'
import type { Account } from '../types/account'
import type { AccountConfigUpdate } from '../services/account-service'
import AccountProxyFields from './AccountProxyFields'
import { apiKeyLines, buildProxyUrl, emptyProxyForm, parseProxyUrl } from '../utils/proxy-config'

interface AccountConfigModalProps {
  open: boolean
  onClose: () => void
  account: Account | null
  onSave: (id: string, updates: AccountConfigUpdate) => Promise<void>
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
  const [groupName, setGroupName] = useState('')
  const [proxyForm, setProxyForm] = useState(emptyProxyForm)
  const [apiKey, setApiKey] = useState('')
  const [errorText, setErrorText] = useState('')

  useEffect(() => {
    if (account) {
      setTier(account.tier)
      setConcurrency(String(account.concurrency_limit))
      setDailyLimit(String(account.daily_limit))
      setTotalLimit(String(account.total_limit))
      setTaskInterval(String(account.task_interval_seconds ?? 0))
      setGroupName(account.group_name || '默认分组')
      setProxyForm(parseProxyUrl(account.proxy_url))
      setApiKey(account.api_key || '')
      setErrorText('')
    }
  }, [account])

  const handleSave = async () => {
    if (!account) return
    let proxyUrl = ''
    try {
      proxyUrl = buildProxyUrl(proxyForm)
    } catch (error) {
      setErrorText(error instanceof Error ? error.message : '代理配置不正确。')
      return
    }
    await onSave(account.id, {
      tier,
      concurrency_limit: toNonNegativeInt(concurrency),
      daily_limit: toNonNegativeInt(dailyLimit),
      total_limit: toNonNegativeInt(totalLimit),
      task_interval_seconds: toNonNegativeInt(taskInterval),
      group_name: groupName.trim() || '默认分组',
      proxy_url: proxyUrl,
      api_key: apiKeyLines(apiKey).join('\n'),
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
          <label style={labelStyle}>账号分组</label>
          <KtInput value={groupName} onChange={(e) => setGroupName(e.target.value)} />
        </div>
        <div style={fieldStyle}>
          <label style={labelStyle}>账号代理</label>
          <AccountProxyFields value={proxyForm} onChange={setProxyForm} />
        </div>
        <div style={fieldStyle}>
          <label style={labelStyle}>授权 API Key</label>
          <textarea
            rows={4}
            placeholder="每行一个 API Key；也可以在设置页用 key=账号组 授权"
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            style={{
              width: '100%',
              resize: 'vertical',
              border: '1px solid var(--border-muted)',
              borderRadius: 8,
              background: 'var(--bg-secondary)',
              color: 'var(--text-primary)',
              padding: 10,
              boxSizing: 'border-box',
            }}
          />
          <span style={{ color: 'var(--text-disabled)', fontSize: 12 }}>
            外部请求携带匹配 Key，或 Key 授权到本账号分组时，才会调度这个账号。
          </span>
        </div>
        {errorText && <div style={{ color: 'var(--color-error)', fontSize: 13 }}>{errorText}</div>}
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
