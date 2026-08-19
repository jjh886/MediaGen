import { useState, useEffect } from 'react'
import { KtModal, KtButton, KtInput, KtSelect } from '@/shared/ui'
import { fetchAllProxies, type ProxyItem } from '@/shared/services/proxy-service'
import type { Account } from '../types/account'
import type { AccountConfigUpdate } from '../services/account-service'
import AccountBrowserProfileFields from './AccountBrowserProfileFields'
import AccountProxyFields from './AccountProxyFields'
import {
  buildBrowserProfilePayload,
  emptyBrowserProfileForm,
  parseBrowserProfile,
  type BrowserProfilePayload,
} from '../utils/browser-profile'
import { accountResetText, accountStateLabel } from '../utils/account-status'
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

function parseIntField(value: string, label: string, min: number, max?: number) {
  const parsed = Number(value)
  if (!Number.isInteger(parsed) || parsed < min || (max !== undefined && parsed > max)) {
    const range = max === undefined ? `不小于 ${min}` : `${min} 到 ${max} 之间`
    throw new Error(`${label}必须是${range}的整数。`)
  }
  return parsed
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
  const [proxies, setProxies] = useState<ProxyItem[]>([])
  const [proxySource, setProxySource] = useState('manual')
  const [proxyForm, setProxyForm] = useState(emptyProxyForm)
  const [proxyDirty, setProxyDirty] = useState(false)
  const [browserProfileForm, setBrowserProfileForm] = useState(emptyBrowserProfileForm)
  const [apiKey, setApiKey] = useState('')
  const [errorText, setErrorText] = useState('')
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    if (account) {
      setTier(account.tier)
      setConcurrency(String(account.concurrency_limit))
      setDailyLimit(String(account.daily_limit))
      setTotalLimit(String(account.total_limit))
      setTaskInterval(String(account.task_interval_seconds ?? 0))
      setProxySource(
        account.proxy_id ? `saved:${account.proxy_id}` : account.proxy_url ? 'manual' : 'none'
      )
      setProxyForm(parseProxyUrl(account.proxy_url))
      setProxyDirty(false)
      setBrowserProfileForm(parseBrowserProfile(account.browser_os, account.browser_version))
      setApiKey('')
      setErrorText('')
    }
  }, [account])

  useEffect(() => {
    if (!open) return
    void fetchAllProxies()
      .then(setProxies)
      .catch(() => setProxies([]))
  }, [open])

  const handleSave = async () => {
    if (!account) return
    let proxyUrl = ''
    let browserProfile: BrowserProfilePayload = { browser_os: '', browser_version: '' }
    try {
      proxyUrl = proxySource === 'manual' ? buildProxyUrl(proxyForm) : ''
      browserProfile = buildBrowserProfilePayload(browserProfileForm)
      const updates = buildUpdates(browserProfile, proxyUrl)
      setSaving(true)
      setErrorText('')
      await onSave(account.id, updates)
      onClose()
    } catch (error) {
      setErrorText(error instanceof Error ? error.message : '账号配置不正确。')
    } finally {
      setSaving(false)
    }
  }

  const buildUpdates = (browserProfile: BrowserProfilePayload, proxyUrl: string) => {
    const updates: AccountConfigUpdate = {
      tier,
      concurrency_limit: parseIntField(concurrency, '并发数', 1, 10),
      daily_limit: parseIntField(dailyLimit, '每日限额', 0),
      total_limit: parseIntField(totalLimit, '总限额', 0),
      task_interval_seconds: parseIntField(taskInterval, '任务间隔', 0),
      browser_os: browserProfile.browser_os,
      browser_version: browserProfile.browser_version,
    }
    if (proxySource === 'none') {
      updates.proxy_url = ''
      updates.proxy_id = ''
    } else if (proxySource.startsWith('saved:')) {
      updates.proxy_url = ''
      updates.proxy_id = proxySource.slice(6)
    } else if (proxyDirty) {
      updates.proxy_url = proxyUrl
      updates.proxy_id = ''
    }
    if (apiKey.trim()) {
      updates.api_key = apiKeyLines(apiKey).join('\n')
    }
    return updates
  }
  const platformStatusText = account ? accountStatusText(account) : ''

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
          <KtButton variant="primary" loading={saving} onClick={handleSave}>
            保存
          </KtButton>
        </>
      }
    >
      <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
        {account && (
          <div style={fieldStyle}>
            <label style={labelStyle}>平台状态</label>
            <span style={{ color: 'var(--text-primary)', fontSize: 13 }}>{platformStatusText}</span>
          </div>
        )}
        <div style={fieldStyle}>
          <label style={labelStyle}>账号代理</label>
          <KtSelect
            value={proxySource}
            onChange={(value) => {
              setProxySource(value)
              setProxyDirty(value !== 'manual')
            }}
            options={proxyOptions(proxies)}
          />
          {proxySource === 'manual' && (
            <AccountProxyFields
              value={proxyForm}
              onChange={(value) => {
                setProxyDirty(true)
                setProxyForm(value)
              }}
            />
          )}
        </div>
        <div style={fieldStyle}>
          <label style={labelStyle}>浏览器环境</label>
          <AccountBrowserProfileFields
            value={browserProfileForm}
            onChange={setBrowserProfileForm}
          />
        </div>
        <div style={fieldStyle}>
          <label style={labelStyle}>授权 API Key</label>
          <textarea
            rows={4}
            placeholder={
              account?.api_key_configured
                ? `${account.api_key_masked || '已保存'}；留空表示不修改`
                : '每行一个 API Key；留空表示不设置单账号授权'
            }
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
            已保存的 Key 不会回显；需要变更时重新填写即可。
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

function accountStatusText(account: Account): string {
  const resetText = accountResetText(account)
  return [accountStateLabel[account.login_state], resetText].filter(Boolean).join(' · ')
}

function proxyOptions(proxies: ProxyItem[]) {
  return [
    { value: 'none', label: '不使用代理' },
    { value: 'manual', label: '手动设置代理' },
    ...proxies
      .filter((proxy) => proxy.enabled)
      .map((proxy) => ({ value: `saved:${proxy.id}`, label: proxy.name || proxy.server })),
  ]
}
