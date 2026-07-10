import { useState, useEffect, useRef, type CSSProperties, type ReactNode } from 'react'
import BrowserHistoryControls from '@/shared/browser/BrowserHistoryControls'
import RemoteBrowserPanel from '@/shared/browser/RemoteBrowserPanel'
import { fetchProxies, type ProxyItem } from '@/shared/services/proxy-service'
import { KtModal, KtButton, KtSelect } from '@/shared/ui'
import {
  browserSessionHistory,
  cancelBrowserSession,
  completeBrowserSession,
  type BrowserHistoryAction,
  type BrowserSession,
} from '@/shared/services/browser-session-service'
import { startAccountLoginSession } from '../services/account-service'
import AccountBrowserProfileFields from './AccountBrowserProfileFields'
import AccountProxyFields from './AccountProxyFields'
import {
  buildBrowserProfilePayload,
  emptyBrowserProfileForm,
  type BrowserProfilePayload,
  type BrowserProfileFormState,
} from '../utils/browser-profile'
import { buildProxyUrl, emptyProxyForm, type ProxyFormState } from '../utils/proxy-config'

interface AccountAddDialogProps {
  open: boolean
  onClose: () => void
  platform: string
  /** 传入 accountId 则为重新登录模式，不传则为添加新账号模式 */
  reloginAccountId?: string | null
  onSuccess: () => void
}

function loginErrorMessage(error: unknown): string {
  if (error instanceof Error && error.message.trim()) return error.message
  if (typeof error === 'string' && error.trim()) return error
  if (error && typeof error === 'object') {
    const record = error as Record<string, unknown>
    for (const key of ['message', 'error']) {
      const value = record[key]
      if (typeof value === 'string' && value.trim()) return value
    }
  }
  return '登录失败，请重试。'
}

export default function AccountAddDialog({
  open,
  onClose,
  platform,
  reloginAccountId,
  onSuccess,
}: AccountAddDialogProps) {
  const [loading, setLoading] = useState(false)
  const [errorText, setErrorText] = useState('')
  const [browserSession, setBrowserSession] = useState<BrowserSession | null>(null)
  const [proxyForm, setProxyForm] = useState<ProxyFormState>(emptyProxyForm)
  const [browserProfileForm, setBrowserProfileForm] =
    useState<BrowserProfileFormState>(emptyBrowserProfileForm)
  const [proxyMode, setProxyMode] = useState('none')
  const [proxies, setProxies] = useState<ProxyItem[]>([])
  const completingRef = useRef(false)
  const cancelledRef = useRef(false)

  const isRelogin = !!reloginAccountId

  useEffect(() => {
    if (!open) {
      setLoading(false)
      cancelledRef.current = true
      completingRef.current = false
      setBrowserSession(null)
      setProxyForm(emptyProxyForm)
      setBrowserProfileForm(emptyBrowserProfileForm)
      setProxyMode(reloginAccountId ? 'keep' : 'none')
    } else {
      cancelledRef.current = false
      setErrorText('')
      void fetchProxies()
        .then(setProxies)
        .catch(() => setProxies([]))
    }
  }, [open])

  const handleStart = async () => {
    let proxyUrl = ''
    let browserProfile: BrowserProfilePayload | undefined
    try {
      proxyUrl = proxyMode === 'manual' ? buildProxyUrl(proxyForm) : ''
      browserProfile = browserProfileForm.enabled
        ? buildBrowserProfilePayload(browserProfileForm)
        : undefined
    } catch (error) {
      setErrorText(loginErrorMessage(error))
      return
    }
    setLoading(true)
    setErrorText('')
    cancelledRef.current = false
    try {
      const selectedProxyId = proxyMode.startsWith('saved:')
        ? proxyMode.slice(6)
        : proxyMode === 'direct'
          ? ''
          : undefined
      const proxyOverride =
        proxyMode === 'keep'
          ? undefined
          : selectedProxyId
            ? ''
            : proxyMode === 'direct'
              ? ''
              : proxyUrl
      const session = await startAccountLoginSession(
        platform,
        reloginAccountId,
        proxyOverride,
        selectedProxyId,
        browserProfile,
        proxyMode === 'direct' ? 'direct' : undefined
      )
      if (cancelledRef.current) {
        await cancelBrowserSession(session.id).catch(() => undefined)
      } else {
        setBrowserSession(session)
      }
      setLoading(false)
    } catch (error) {
      if (!cancelledRef.current) {
        setErrorText(loginErrorMessage(error))
        setLoading(false)
      }
    }
  }

  const handleProxyModeChange = (value: string) => {
    setProxyMode(value)
    if (value === 'manual') {
      setProxyForm((current) => ({ ...current, enabled: true }))
    }
  }

  const handleCancel = (cancelRemote = true) => {
    cancelledRef.current = true
    const sessionId = browserSession?.id
    if (cancelRemote && sessionId) {
      void cancelBrowserSession(sessionId).catch(() => undefined)
    }
    setLoading(false)
    setBrowserSession(null)
    onClose()
  }
  const handleCancelClick = () => handleCancel(true)

  const handleHistory = async (action: BrowserHistoryAction) => {
    if (!browserSession) return
    await browserSessionHistory(browserSession.id, action).catch(() => undefined)
  }

  useEffect(() => {
    if (!browserSession) return
    const timer = window.setInterval(async () => {
      if (cancelledRef.current || completingRef.current) return
      completingRef.current = true
      try {
        await completeBrowserSession(browserSession.id)
        onSuccess()
        handleCancel(false)
      } catch {
        completingRef.current = false
      }
    }, 3000)
    return () => window.clearInterval(timer)
  }, [browserSession, onSuccess])

  const title = isRelogin ? '重新登录' : '添加账号'
  const loadingText = isRelogin ? '正在创建重新登录页面...' : '正在创建登录页面...'
  const hintText = isRelogin
    ? '请在页面中重新登录，检测成功后窗口会自动关闭'
    : '请在页面中完成登录，检测成功后窗口会自动关闭'

  return (
    <KtModal
      open={open}
      onClose={handleCancelClick}
      title={title}
      titleExtra={browserSession ? <BrowserHistoryControls onAction={handleHistory} /> : null}
      width="min(1180px, calc(100vw - 32px))"
      height="min(860px, calc(100vh - 32px))"
      bodyStyle={{ padding: 0, overflow: 'hidden' }}
      resizable
      closeOnOverlayClick={false}
      footer={
        browserSession ? undefined : loading ? (
          <KtButton variant="danger" onClick={handleCancelClick}>
            取消等待
          </KtButton>
        ) : (
          <>
            <KtButton variant="default" onClick={handleCancelClick}>
              取消
            </KtButton>
            <KtButton variant="primary" onClick={handleStart}>
              打开登录页
            </KtButton>
          </>
        )
      }
    >
      <div style={{ position: 'relative', height: '100%', minHeight: 0, textAlign: 'center' }}>
        {browserSession ? (
          <>
            <RemoteBrowserPanel session={browserSession} mode="login" />
            {errorText && (
              <div
                style={{
                  position: 'absolute',
                  left: 24,
                  right: 24,
                  bottom: 80,
                  color: 'var(--color-error)',
                  fontSize: 13,
                  background: 'rgba(20,20,20,0.92)',
                  border: '1px solid var(--color-error)',
                  borderRadius: 8,
                  padding: '10px 12px',
                }}
              >
                {errorText}
              </div>
            )}
          </>
        ) : loading ? (
          <div style={{ display: 'grid', placeItems: 'center', height: '100%' }}>
            <div
              style={{
                width: 32,
                height: 32,
                border: '3px solid rgba(255,255,255,0.1)',
                borderTopColor: 'var(--color-brand)',
                borderRadius: '50%',
                animation: 'kt-spin 0.8s linear infinite',
                margin: '0 auto 16px',
              }}
            />
            <div style={{ color: 'var(--text-primary)', fontSize: 14 }}>{loadingText}</div>
            <div style={{ color: 'var(--text-secondary)', fontSize: 13, marginTop: 8 }}>
              {hintText}
            </div>
            <div style={{ color: 'var(--text-disabled)', fontSize: 12, marginTop: 12 }}>
              点击「取消等待」可关闭当前登录流程
            </div>
            <style>{`@keyframes kt-spin { to { transform: rotate(360deg); } }`}</style>
          </div>
        ) : (
          <div style={{ padding: 24, textAlign: 'left' }}>
            <div style={{ color: 'var(--text-primary)', fontSize: 14, marginBottom: 14 }}>
              {isRelogin
                ? '重新登录时默认沿用账号当前代理和浏览器环境；需要临时调整时再修改下面的选项。'
                : '登录前可以给这个账号绑定独立代理和浏览器环境；不设置则使用服务器默认环境。'}
            </div>
            <SavedProxyPicker
              value={proxyMode}
              proxies={proxies}
              isRelogin={isRelogin}
              onChange={handleProxyModeChange}
            />
            {proxyMode === 'manual' && (
              <AccountProxyFields value={proxyForm} onChange={setProxyForm} />
            )}
            <div style={{ marginTop: 16 }}>
              <AccountBrowserProfileFields
                value={browserProfileForm}
                onChange={setBrowserProfileForm}
              />
            </div>
            {errorText && (
              <div style={{ color: 'var(--color-error)', fontSize: 13, marginTop: 12 }}>
                {errorText}
              </div>
            )}
          </div>
        )}
      </div>
    </KtModal>
  )
}

function SavedProxyPicker({
  value,
  proxies,
  isRelogin,
  onChange,
}: {
  value: string
  proxies: ProxyItem[]
  isRelogin: boolean
  onChange: (value: string) => void
}) {
  return (
    <div style={pickerStyle}>
      <PickerLabel id="account-proxy-source">账号代理</PickerLabel>
      <KtSelect
        id="account-proxy-source"
        aria-label="账号代理"
        value={value}
        onChange={onChange}
        options={proxyModeOptions(isRelogin, proxies)}
      />
      <ProxyModeHint value={value} proxies={proxies} />
    </div>
  )
}

function proxyModeOptions(isRelogin: boolean, proxies: ProxyItem[]) {
  const base = isRelogin
    ? [
        { value: 'keep', label: '沿用账号当前代理' },
        { value: 'direct', label: '本次不使用代理' },
      ]
    : [{ value: 'none', label: '不使用代理' }]
  const saved = proxies
    .filter((proxy) => proxy.enabled)
    .map((proxy) => ({
      value: `saved:${proxy.id}`,
      label: `使用保存代理：${proxy.name || proxy.server}`,
    }))
  return [...base, ...saved, { value: 'manual', label: '手动填写代理' }]
}

function ProxyModeHint({ value, proxies }: { value: string; proxies: ProxyItem[] }) {
  if (value.startsWith('saved:')) return <Hint>将使用代理管理页中保存的地址、账号和密码。</Hint>
  if (value === 'manual') return <Hint>下面填写的代理只绑定到这个账号。</Hint>
  if (value === 'direct') return <Hint>本次登录不会沿用账号已保存的代理。</Hint>
  if (!proxies.some((proxy) => proxy.enabled)) {
    return <Hint>右侧代理页新增可用代理后，会直接出现在这里。</Hint>
  }
  if (value === 'none') return <Hint>这个账号会使用服务器默认网络。</Hint>
  return null
}

function PickerLabel({ id, children }: { id: string; children: ReactNode }) {
  return (
    <label htmlFor={id} style={{ color: 'var(--text-secondary)', fontSize: 13 }}>
      {children}
    </label>
  )
}

function Hint({ children }: { children: ReactNode }) {
  return <span style={{ color: 'var(--text-disabled)', fontSize: 12 }}>{children}</span>
}

const pickerStyle: CSSProperties = {
  display: 'flex',
  flexDirection: 'column',
  gap: 6,
  marginBottom: 14,
}
