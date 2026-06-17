import { useEffect, useState } from 'react'
import { AlertCircle, CheckCircle2, Chrome, RefreshCw, Settings } from 'lucide-react'
import { KtButton } from '@/shared/ui'
import {
  checkStartupReadiness,
  confirmMacosAppManagementPermission,
  openMacosPrivacySettings,
  requestMacosStartupPermissions,
  type StartupReadiness,
} from '@/shared/services/startup-service'

interface StartupGateProps {
  children: React.ReactNode
}

type GateStatus = 'checking' | 'blocked' | 'ready'

export function StartupGate({ children }: StartupGateProps) {
  const [status, setStatus] = useState<GateStatus>('checking')
  const [block, setBlock] = useState<StartupReadiness | null>(null)
  const [busy, setBusy] = useState(false)

  const runCheck = async () => {
    setStatus('checking')
    const result = await checkStartupReadiness()
    if (result.ok) {
      setBlock(null)
      setStatus('ready')
      return
    }
    setBlock(result)
    setStatus('blocked')
  }

  useEffect(() => {
    void runCheck()
  }, [])

  if (status === 'ready') return <>{children}</>
  if (status === 'checking') return <StartupCheckingScreen />

  return (
    <StartupBlockScreen
      block={block}
      busy={busy}
      onRetry={runCheck}
      onOpenSettings={async () => {
        setBusy(true)
        try {
          await requestMacosStartupPermissions()
          await openMacosPrivacySettings()
        } finally {
          setBusy(false)
        }
      }}
      onConfirmAuthorized={() => {
        confirmMacosAppManagementPermission()
        void runCheck()
      }}
    />
  )
}

function StartupCheckingScreen() {
  return (
    <div className="startup-screen">
      <div className="startup-panel startup-panel--compact">
        <RefreshCw className="startup-spinner" size={24} aria-hidden />
        <h1>正在检查运行环境</h1>
        <p>MediaGen 正在确认浏览器和系统权限，检查通过后会自动进入应用。</p>
      </div>
    </div>
  )
}

function StartupBlockScreen({
  block,
  busy,
  onRetry,
  onOpenSettings,
  onConfirmAuthorized,
}: {
  block: StartupReadiness | null
  busy: boolean
  onRetry: () => void
  onOpenSettings: () => void
  onConfirmAuthorized: () => void
}) {
  const isPermissionBlock = block?.reason === 'macos-permission'

  return (
    <div className="startup-screen">
      <div className="startup-panel">
        <div className="startup-panel__icon" aria-hidden>
          {isPermissionBlock ? <Settings size={28} /> : <Chrome size={28} />}
        </div>
        <h1>{block?.title ?? '启动检查未通过'}</h1>
        <p>{block?.message ?? '请处理下面的问题后重新检测。'}</p>

        {isPermissionBlock && <PermissionChecklist permissions={block?.permissions ?? []} />}
        {isPermissionBlock && <MacosGuide />}

        <div className="startup-actions">
          {isPermissionBlock && (
            <KtButton variant="primary" onClick={onOpenSettings} loading={busy}>
              <Settings size={16} />
              打开 App 管理设置
            </KtButton>
          )}
          {isPermissionBlock ? (
            <KtButton variant="default" onClick={onConfirmAuthorized}>
              <CheckCircle2 size={16} />
              我已授权并重启
            </KtButton>
          ) : (
            <KtButton variant="primary" onClick={onRetry}>
              <RefreshCw size={16} />
              重新检测
            </KtButton>
          )}
        </div>
      </div>
    </div>
  )
}

function PermissionChecklist({
  permissions,
}: {
  permissions: NonNullable<StartupReadiness['permissions']>
}) {
  return (
    <div className="startup-checklist">
      {permissions.map((item) => (
        <div className="startup-checklist__item" key={item.key}>
          {item.granted ? (
            <CheckCircle2 className="startup-checklist__ok" size={18} aria-hidden />
          ) : (
            <AlertCircle className="startup-checklist__warn" size={18} aria-hidden />
          )}
          <div>
            <strong>{item.title}</strong>
            <span>{item.message}</span>
          </div>
        </div>
      ))}
    </div>
  )
}

function MacosGuide() {
  return (
    <div className="startup-guide">
      <strong>授权后请完全退出并重新打开 MediaGen。</strong>
      <span>
        请在“系统设置 → 隐私与安全性 → App 管理”里打开 MediaGen。系统提示重启生效时，请先重启
        MediaGen，再点“我已授权并重启”。
      </span>
    </div>
  )
}
