import { useState, useEffect, useRef } from 'react'
import { KtModal, KtButton } from '@/shared/ui'
import { addAccountByLogin, reloginAccount } from '../services/account-service'

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
  const cancelledRef = useRef(false)

  const isRelogin = !!reloginAccountId

  useEffect(() => {
    if (!open) {
      setLoading(false)
      cancelledRef.current = true
    } else {
      cancelledRef.current = false
      setErrorText('')
    }
  }, [open])

  const handleStart = async () => {
    setLoading(true)
    setErrorText('')
    cancelledRef.current = false
    try {
      if (isRelogin) {
        await reloginAccount(reloginAccountId!)
      } else {
        const result = await addAccountByLogin(platform)
        if (cancelledRef.current) return
        if (!result) {
          await onSuccess()
          setErrorText('登录未完成，账号未保存。请重新点击开始登录。')
          setLoading(false)
          return
        }
      }
      if (cancelledRef.current) return
      onSuccess()
      onClose()
    } catch (error) {
      if (!cancelledRef.current) {
        setErrorText(loginErrorMessage(error))
        setLoading(false)
      }
    }
  }

  const handleCancel = () => {
    cancelledRef.current = true
    setLoading(false)
    onClose()
  }

  const title = isRelogin ? '重新登录' : '添加账号'
  const actionText = isRelogin ? '打开浏览器' : '开始登录'
  const loadingText = isRelogin ? '正在等待重新登录...' : '正在等待登录...'
  const hintText = isRelogin
    ? '请在弹出的浏览器窗口中重新登录，登录成功后窗口会自动关闭'
    : '请在弹出的浏览器窗口中完成登录，登录成功后窗口会自动关闭'

  return (
    <KtModal
      open={open}
      onClose={handleCancel}
      title={title}
      footer={
        loading ? (
          <KtButton variant="danger" onClick={handleCancel}>
            取消等待
          </KtButton>
        ) : (
          <>
            <KtButton variant="default" onClick={handleCancel}>
              取消
            </KtButton>
            <KtButton variant="primary" onClick={handleStart}>
              {actionText}
            </KtButton>
          </>
        )
      }
    >
      <div style={{ textAlign: 'center', padding: '24px 0' }}>
        {loading ? (
          <>
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
              点击「取消等待」可关闭弹窗（浏览器窗口仍在运行，登录成功后会自动保存）
            </div>
            <style>{`@keyframes kt-spin { to { transform: rotate(360deg); } }`}</style>
          </>
        ) : (
          <div style={{ color: 'var(--text-secondary)', fontSize: 14 }}>
            {isRelogin
              ? `点击「${actionText}」将打开浏览器窗口，请在其中重新完成 ${platform} 账号登录。`
              : `点击「${actionText}」将打开独立浏览器窗口，请在其中完成 ${platform} 账号登录。`}
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
