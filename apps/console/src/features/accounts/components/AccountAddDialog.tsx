import { useState, useEffect, useRef, type CSSProperties } from 'react'
import { ArrowLeft, ArrowRight, RotateCw } from 'lucide-react'
import { KtModal, KtButton } from '@/shared/ui'
import {
  browserSessionHistory,
  cancelBrowserSession,
  completeBrowserSession,
  type BrowserHistoryAction,
  type BrowserSession,
} from '@/shared/services/browser-session-service'
import RemoteBrowserPanel from '@/shared/browser/RemoteBrowserPanel'
import { startAccountLoginSession } from '../services/account-service'
import AccountProxyFields from './AccountProxyFields'
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
    } else {
      cancelledRef.current = false
      setErrorText('')
    }
  }, [open])

  const handleStart = async () => {
    let proxyUrl = ''
    try {
      proxyUrl = buildProxyUrl(proxyForm)
    } catch (error) {
      setErrorText(loginErrorMessage(error))
      return
    }
    setLoading(true)
    setErrorText('')
    cancelledRef.current = false
    try {
      const proxyOverride = !isRelogin || proxyForm.enabled ? proxyUrl : undefined
      const session = await startAccountLoginSession(platform, reloginAccountId, proxyOverride)
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
      titleExtra={
        browserSession ? (
          <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
            <button
              type="button"
              title="后退"
              aria-label="后退"
              onClick={() => handleHistory('back')}
              style={historyButtonStyle}
            >
              <ArrowLeft size={16} />
            </button>
            <button
              type="button"
              title="前进"
              aria-label="前进"
              onClick={() => handleHistory('forward')}
              style={historyButtonStyle}
            >
              <ArrowRight size={16} />
            </button>
            <button
              type="button"
              title="刷新"
              aria-label="刷新"
              onClick={() => handleHistory('reload')}
              style={historyButtonStyle}
            >
              <RotateCw size={16} />
            </button>
          </div>
        ) : null
      }
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
                ? '重新登录时默认沿用账号当前代理；需要临时换代理时再打开下面的开关。'
                : '登录前可以给这个账号绑定独立代理；不设置则使用服务器直连。'}
            </div>
            <AccountProxyFields value={proxyForm} onChange={setProxyForm} />
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

const historyButtonStyle: CSSProperties = {
  width: 30,
  height: 30,
  display: 'inline-flex',
  alignItems: 'center',
  justifyContent: 'center',
  border: '1px solid rgba(255,255,255,0.12)',
  borderRadius: 6,
  background: 'rgba(255,255,255,0.06)',
  color: '#fff',
  cursor: 'pointer',
  padding: 0,
}
