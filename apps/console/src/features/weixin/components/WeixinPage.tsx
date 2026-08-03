import { useCallback, useEffect, useMemo, useState } from 'react'
import { RefreshCw, Save } from 'lucide-react'
import { KtAsyncState, KtButton, KtConfirmDialog } from '@/shared/ui'
import { useWeixinSettings } from '../hooks/useWeixinSettings'
import type { BindingActionResult } from '../types/weixin'
import { WeixinBindingDialog } from './WeixinBindingDialog'
import { WeixinBindingPanel } from './WeixinBindingPanel'
import { WeixinGenerationPanel } from './WeixinGenerationPanel'
import { WeixinNotificationPanel } from './WeixinNotificationPanel'
import { pageStyle, panelGridStyle } from './WeixinShared'

export default function WeixinPage() {
  const {
    settings,
    draft,
    loading,
    saving,
    acting,
    error,
    message,
    load,
    updateDraft,
    save,
    action,
    beginBinding,
  } = useWeixinSettings()
  const [bindingOpen, setBindingOpen] = useState(false)
  const [bindingResult, setBindingResult] = useState<BindingActionResult | null>(null)
  const [removeOpen, setRemoveOpen] = useState(false)

  useEffect(() => {
    void load()
  }, [load])

  const dirty = useMemo(() => {
    if (!settings || !draft) return false
    return (
      draft.generationEnabled !== settings.generationEnabled ||
      draft.directPromptEnabled !== settings.directPromptEnabled ||
      draft.defaultModelId !== settings.defaultModelId ||
      draft.defaultAspectRatio !== settings.defaultAspectRatio ||
      draft.defaultMaxImages !== settings.defaultMaxImages ||
      draft.proactiveNotificationsEnabled !== settings.proactiveNotificationsEnabled ||
      draft.notifyDesktopTasks !== settings.notifyDesktopTasks ||
      draft.notifyTaskSuccesses !== settings.notifyTaskSuccesses ||
      draft.notifyNoAccounts !== settings.notifyNoAccounts ||
      draft.notifyAccountActions !== settings.notifyAccountActions ||
      draft.quietHoursJson !== settings.quietHoursJson
    )
  }, [draft, settings])

  const startBinding = async () => {
    const result = await beginBinding()
    if (!result) return
    setBindingResult(result)
    setBindingOpen(true)
  }

  const pollBinding = useCallback(
    async (sessionId: string) => {
      const result = await action({ action: 'poll_binding', sessionId })
      if (!result) return
      setBindingResult((current) => ({
        ...result,
        qrImage: result.qrImage || current?.qrImage,
      }))
    },
    [action]
  )

  const submitCode = useCallback(
    async (sessionId: string, value: string) => {
      const result = await action({ action: 'submit_verify_code', sessionId, value })
      if (result) setBindingResult(result)
    },
    [action]
  )

  const cancelBinding = useCallback(
    async (sessionId?: string) => {
      if (sessionId) await action({ action: 'cancel_binding', sessionId })
      setBindingOpen(false)
      setBindingResult(null)
    },
    [action]
  )

  const sendTest = async () => {
    await action({ action: 'send_test' })
  }

  const removeBinding = async () => {
    const result = await action({ action: 'remove_binding' })
    if (result) setRemoveOpen(false)
  }

  if (loading && !settings) {
    return <KtAsyncState loading error="" empty={false} />
  }

  if (!settings || !draft) {
    return (
      <div style={{ padding: 32 }}>
        <KtAsyncState
          loading={false}
          error={error || '微信设置加载失败。'}
          empty={false}
          onRetry={load}
        />
      </div>
    )
  }

  return (
    <div style={pageStyle}>
      <div
        style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 12 }}
      >
        <div>
          <h1 style={{ color: 'var(--text-bright)', fontSize: 20, lineHeight: '28px' }}>微信</h1>
          <p style={{ color: 'var(--text-secondary)', fontSize: 13, marginTop: 4 }}>
            绑定后可在微信里排队生成图片，并接收本机任务和账号状态通知。
          </p>
        </div>
        <div style={{ display: 'flex', gap: 8 }}>
          <KtButton size="sm" onClick={() => void load()} loading={loading}>
            <RefreshCw size={14} />
            刷新
          </KtButton>
          <KtButton
            size="sm"
            variant="primary"
            disabled={!dirty}
            loading={saving}
            onClick={() => void save()}
          >
            <Save size={14} />
            保存
          </KtButton>
        </div>
      </div>

      {(error || message) && (
        <div
          role={error ? 'alert' : 'status'}
          style={{
            color: error ? 'var(--color-error)' : 'var(--color-success)',
            fontSize: 13,
          }}
        >
          {error || message}
        </div>
      )}

      <div style={panelGridStyle}>
        <WeixinBindingPanel
          settings={settings}
          acting={acting}
          onStartBinding={startBinding}
          onSendTest={sendTest}
          onRemoveBinding={() => setRemoveOpen(true)}
        />
        <div style={{ display: 'flex', flexDirection: 'column', gap: 16, minWidth: 0 }}>
          <WeixinGenerationPanel settings={settings} draft={draft} onChange={updateDraft} />
          <WeixinNotificationPanel draft={draft} onChange={updateDraft} />
        </div>
      </div>

      <WeixinBindingDialog
        open={bindingOpen}
        result={bindingResult}
        acting={acting}
        onPoll={pollBinding}
        onSubmitCode={submitCode}
        onCancel={cancelBinding}
        onClose={() => setBindingOpen(false)}
      />
      <KtConfirmDialog
        open={removeOpen}
        title="移除微信绑定"
        message="移除后将停止微信指令、生图结果回传和主动通知。已排队的普通图片任务不会被删除。"
        confirmText="移除"
        danger
        loading={acting}
        onCancel={() => setRemoveOpen(false)}
        onConfirm={removeBinding}
      />
    </div>
  )
}
