import { useState, useEffect } from 'react'
import { KtModal, KtButton, KtInput, KtSelect } from '@/shared/ui'
import { fetchAllProxies, type ProxyItem } from '@/shared/services/proxy-service'
import type { Account } from '../types/account'
import {
  fetchAccountModels,
  type AccountModelCatalog,
  type AccountConfigUpdate,
} from '../services/account-service'
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
  const [priority, setPriority] = useState('0')
  const [textConcurrency, setTextConcurrency] = useState('0')
  const [textDaily, setTextDaily] = useState('0')
  const [textTotal, setTextTotal] = useState('0')
  const [catalog, setCatalog] = useState<AccountModelCatalog | null>(null)
  const [modelsLoading, setModelsLoading] = useState(false)
  const [modelsError, setModelsError] = useState('')
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
      setPriority(String(account.priority ?? 0))
      setTextConcurrency(String(account.text_concurrency_limit ?? 0))
      setTextDaily(String(account.text_daily_limit ?? 0))
      setTextTotal(String(account.text_total_limit ?? 0))
      setCatalog(null)
      setModelsError('')
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
  const loadModels = async () => {
    if (!account) return
    setModelsLoading(true)
    setModelsError('')
    try {
      const result = await fetchAccountModels(account.id, true)
      setCatalog(result)
      if (result.refresh_error)
        setModelsError(`刷新失败（${result.refresh_error}），保留上次目录。`)
    } catch (error) {
      setModelsError(error instanceof Error ? error.message : '模型目录获取失败')
    } finally {
      setModelsLoading(false)
    }
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
          <KtButton loading={modelsLoading} onClick={() => void loadModels()}>
            获取账号支持的模型
          </KtButton>
          {modelsError && <span role="alert">{modelsError}</span>}
          {catalog && !catalog.discovery_supported && <span>此插件提供随版本发布的模型目录。</span>}
          {catalog?.models.map((model) => (
            <details key={model.id}>
              <summary>
                {model.display_name}（{model.id}）
              </summary>
              <p>
                输入：{model.capabilities.input_modalities?.join('、') || '见插件参数'}；输出：
                {model.capabilities.output_modalities?.join('、') || model.task_kind}
              </p>
              <p>接口：{model.capabilities.endpoints?.join('、') || '见插件说明'}</p>
              {model.capabilities.unsupported_parameters?.length ? (
                <p>不支持的参数：{model.capabilities.unsupported_parameters.join('、')}</p>
              ) : null}
              <pre style={{ whiteSpace: 'pre-wrap', overflowWrap: 'anywhere' }}>
                {JSON.stringify(model.capabilities.input_schema, null, 2)}
              </pre>
            </details>
          ))}
        </div>
        {[
          { label: '调度优先级（数字越小越优先）', value: priority, setter: setPriority },
          { label: '文本并发数（0 为不限）', value: textConcurrency, setter: setTextConcurrency },
          { label: '文本每日限额（0 为不限）', value: textDaily, setter: setTextDaily },
          { label: '文本总限额（0 为不限）', value: textTotal, setter: setTextTotal },
        ].map((field) => (
          <div key={field.label} style={fieldStyle}>
            <label style={labelStyle}>{field.label}</label>
            <KtInput
              aria-label={field.label}
              type="number"
              min={0}
              value={field.value}
              onChange={(event) => field.setter(event.target.value)}
            />
          </div>
        ))}
        {account && (
          <span>
            文本用量：今日 {account.text_used_today ?? 0}，累计 {account.text_used_total ?? 0}
            ；最近检查：
            {account.last_check_at ? new Date(account.last_check_at).toLocaleString() : '尚未检查'}
          </span>
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
          <label style={labelStyle}>图片/媒体并发数</label>
          <KtInput
            type="number"
            min={0}
            max={10}
            value={concurrency}
            onChange={(e) => setConcurrency(e.target.value)}
          />
        </div>
        <div style={fieldStyle}>
          <label style={labelStyle}>图片/媒体每日限额</label>
          <KtInput
            type="number"
            min={0}
            value={dailyLimit}
            onChange={(e) => setDailyLimit(e.target.value)}
          />
        </div>
        <div style={fieldStyle}>
          <label style={labelStyle}>图片/媒体总限额</label>
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
