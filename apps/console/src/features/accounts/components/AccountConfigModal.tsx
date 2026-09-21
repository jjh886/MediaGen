import { useState, useEffect, useRef } from 'react'
import AccountQuotaSummary from './AccountQuotaSummary'
import { KtModal, KtButton, KtInput, KtSelect } from '@/shared/ui'
import { fetchAllProxies, proxyOptionLabel, type ProxyItem } from '@/shared/services/proxy-service'
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
  onRefreshQuota?: () => void
  quotaRefreshing?: boolean
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
  onRefreshQuota,
  quotaRefreshing,
}: AccountConfigModalProps) {
  const [priority, setPriority] = useState('0')
  const initialized = useRef('')
  const [textConcurrency, setTextConcurrency] = useState('0')
  const [textDaily, setTextDaily] = useState('0')
  const [textTotal, setTextTotal] = useState('0')
  const [tab, setTab] = useState('limits')
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
    if (!open) {
      initialized.current = ''
      return
    }
    if (initialized.current === account?.id) return
    if (account) {
      initialized.current = account.id
      setPriority(String(account.priority ?? 0))
      setTextConcurrency(String(account.text_concurrency_limit ?? 0))
      setTextDaily(String(account.text_daily_limit ?? 0))
      setTextTotal(String(account.text_total_limit ?? 0))
      setTab('limits')
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
  }, [account, open])

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
      priority: parseIntField(priority, '调度优先级', 0),
      text_concurrency_limit: parseIntField(textConcurrency, '文本并发数', 0),
      text_daily_limit: parseIntField(textDaily, '文本每日限额', 0),
      text_total_limit: parseIntField(textTotal, '文本总限额', 0),
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
      width={760}
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
      <div style={{ display: 'grid', gap: 20 }}>
        <div
          style={{
            padding: 16,
            borderRadius: 10,
            background: 'var(--bg-secondary)',
            display: 'grid',
            gap: 6,
          }}
        >
          <strong>{account?.name}</strong>
          <span style={labelStyle}>{platformStatusText}</span>
          <span style={labelStyle}>
            最近检查：
            {account?.last_check_at ? new Date(account.last_check_at).toLocaleString() : '尚未检查'}
          </span>
        </div>
        <div role="tablist" aria-label="账号配置分类" style={{ display: 'flex', gap: 8 }}>
          {[
            ['limits', '额度与调度'],
            ['network', '代理与浏览器'],
            ['access', '账号与授权'],
          ].map(([id, label]) => (
            <KtButton
              key={id}
              role="tab"
              aria-selected={tab === id}
              variant={tab === id ? 'primary' : 'default'}
              onClick={() => setTab(id)}
            >
              {label}
            </KtButton>
          ))}
        </div>
        {tab === 'limits' && (
          <div role="tabpanel" style={{ display: 'grid', gap: 18 }}>
            {account && <AccountQuotaSummary account={account} />}
            {onRefreshQuota && (
              <KtButton size="sm" onClick={onRefreshQuota} loading={quotaRefreshing}>
                刷新额度
              </KtButton>
            )}

            <div style={{ overflowX: 'auto' }}>
              <table style={{ width: '100%', borderSpacing: '0 10px', textAlign: 'left' }}>
                <thead>
                  <tr>
                    <th>能力</th>
                    <th>并发</th>
                    <th>每日限额</th>
                    <th>累计限额</th>
                  </tr>
                </thead>
                <tbody>
                  {[
                    {
                      label: '文本',
                      fields: [
                        [textConcurrency, setTextConcurrency],
                        [textDaily, setTextDaily],
                        [textTotal, setTextTotal],
                      ] as const,
                      used: `今日 ${account?.text_used_today ?? 0} · 累计 ${account?.text_used_total ?? 0}`,
                    },
                    {
                      label: '图片 / 媒体',
                      fields: [
                        [concurrency, setConcurrency],
                        [dailyLimit, setDailyLimit],
                        [totalLimit, setTotalLimit],
                      ] as const,
                      used: `今日 ${account?.used_today ?? 0} · 累计 ${account?.used_total ?? 0}`,
                    },
                  ].map((row) => (
                    <tr key={row.label}>
                      <td style={{ paddingRight: 12, minWidth: 130 }}>
                        <strong>{row.label}</strong>
                        <div style={{ ...labelStyle, fontSize: 11, marginTop: 5 }}>{row.used}</div>
                      </td>
                      {row.fields.map(([value, set], i) => (
                        <td key={i} style={{ paddingRight: 8 }}>
                          <KtInput
                            aria-label={`${row.label}${['并发', '每日限额', '累计限额'][i]}`}
                            type="number"
                            min={row.label === '图片 / 媒体' && i === 0 ? 1 : 0}
                            value={value}
                            onChange={(e) => set(e.target.value)}
                          />
                        </td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <div style={labelStyle}>
              每日及累计限额为 0 表示不限；文本并发为 0
              表示不限。两类用量独立计算，平台额度另行生效。
            </div>
            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 16 }}>
              <label style={fieldStyle}>
                调度优先级
                <KtInput
                  aria-label="调度优先级"
                  type="number"
                  min={0}
                  value={priority}
                  onChange={(e) => setPriority(e.target.value)}
                />
                <small style={labelStyle}>数字越小越优先；已有对话优先沿用原账号。</small>
              </label>
              <label style={fieldStyle}>
                任务间隔（秒）
                <KtInput
                  aria-label="任务间隔"
                  type="number"
                  min={0}
                  value={taskInterval}
                  onChange={(e) => setTaskInterval(e.target.value)}
                />
                <small style={labelStyle}>任务结束后的最短等待时间。</small>
              </label>
            </div>
          </div>
        )}
        {tab === 'network' && (
          <div role="tabpanel" style={{ display: 'grid', gap: 22 }}>
            <div style={fieldStyle}>
              <label style={labelStyle}>账号代理</label>
              <KtSelect
                aria-label="账号代理"
                value={proxySource}
                onChange={(value) => {
                  setProxySource(value)
                  setProxyDirty(value !== 'manual')
                }}
                options={proxyOptions(proxies)}
              />
            </div>
            {proxySource === 'manual' && (
              <AccountProxyFields
                value={proxyForm}
                onChange={(value) => {
                  setProxyDirty(true)
                  setProxyForm(value)
                }}
              />
            )}
            <div style={labelStyle}>
              绑定代理后，登录、额度检查和生成请求均使用该代理；代理失效会报错，不会自动改成直连。
            </div>
            <details>
              <summary style={{ cursor: 'pointer', marginBottom: 12 }}>浏览器指纹设置</summary>
              <AccountBrowserProfileFields
                value={browserProfileForm}
                onChange={setBrowserProfileForm}
              />
            </details>
          </div>
        )}
        {tab === 'access' && (
          <div role="tabpanel" style={{ display: 'grid', gap: 18 }}>
            <div style={fieldStyle}>
              <label style={labelStyle}>账号等级</label>
              <KtSelect
                value={tier}
                onChange={setTier}
                options={
                  tierOptions.some((item) => item.value === tier)
                    ? tierOptions
                    : [{ label: tier, value: tier }, ...tierOptions]
                }
              />
            </div>
            <label style={fieldStyle}>
              授权 API Key
              <textarea
                aria-label="授权 API Key"
                rows={4}
                placeholder={
                  account?.api_key_configured ? '留空保留已保存的 Key' : '每行一个 API Key'
                }
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
                style={{
                  width: '100%',
                  boxSizing: 'border-box',
                  padding: 12,
                  borderRadius: 8,
                  background: 'var(--bg-secondary)',
                  color: 'var(--text-primary)',
                  border: '1px solid var(--border-muted)',
                }}
              />
            </label>
            <span style={labelStyle}>已保存的 Key 不回显；留空不会修改。</span>
          </div>
        )}
        {errorText && (
          <div role="alert" style={{ color: 'var(--color-error)' }}>
            {errorText}
          </div>
        )}
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
      .sort((a, b) => (a.bound_account_count ?? 0) - (b.bound_account_count ?? 0))
      .map((proxy) => ({ value: `saved:${proxy.id}`, label: proxyOptionLabel(proxy) })),
  ]
}
