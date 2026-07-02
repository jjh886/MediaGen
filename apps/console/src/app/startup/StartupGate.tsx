import { useEffect, useState } from 'react'
import { Monitor, RefreshCw } from 'lucide-react'
import { KtButton } from '@/shared/ui'
import {
  checkStartupReadiness,
  getBrowserRuntimeInstallGuide,
  installBrowserRuntime,
  type BrowserRuntimeInstallGuide,
  type StartupReadiness,
} from '@/shared/services/startup-service'

interface StartupGateProps {
  children: React.ReactNode
}

type GateStatus = 'checking' | 'blocked' | 'ready'

export function StartupGate({ children }: StartupGateProps) {
  const [status, setStatus] = useState<GateStatus>('checking')
  const [block, setBlock] = useState<StartupReadiness | null>(null)
  const [installGuide, setInstallGuide] = useState<BrowserRuntimeInstallGuide | null>(null)
  const [installingRuntime, setInstallingRuntime] = useState(false)
  const [runtimeInstallError, setRuntimeInstallError] = useState('')

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
    if (result.reason === 'browser-runtime') {
      setInstallGuide(await getBrowserRuntimeInstallGuide())
    }
  }

  useEffect(() => {
    void runCheck()
  }, [])

  if (status === 'ready') return <>{children}</>
  if (status === 'checking') return <StartupCheckingScreen />

  return (
    <StartupBlockScreen
      block={block}
      installGuide={installGuide}
      installingRuntime={installingRuntime}
      runtimeInstallError={runtimeInstallError}
      onRetry={runCheck}
      onInstallRuntime={async () => {
        setInstallingRuntime(true)
        setRuntimeInstallError('')
        try {
          await installBrowserRuntime()
          await runCheck()
        } catch (error) {
          setRuntimeInstallError(String(error))
        } finally {
          setInstallingRuntime(false)
        }
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
        <p>MediaGen 正在确认服务端浏览器运行时，检查通过后会自动进入应用。</p>
      </div>
    </div>
  )
}

function StartupBlockScreen({
  block,
  installGuide,
  installingRuntime,
  runtimeInstallError,
  onRetry,
  onInstallRuntime,
}: {
  block: StartupReadiness | null
  installGuide: BrowserRuntimeInstallGuide | null
  installingRuntime: boolean
  runtimeInstallError: string
  onRetry: () => void
  onInstallRuntime: () => void
}) {
  return (
    <div className="startup-screen">
      <div className="startup-panel">
        <div className="startup-panel__icon" aria-hidden>
          <Monitor size={28} />
        </div>
        <h1>{block?.title ?? '启动检查未通过'}</h1>
        <p>{block?.message ?? '请处理下面的问题后重新检测。'}</p>

        {installGuide && <RuntimeInstallGuide guide={installGuide} />}
        {runtimeInstallError && (
          <div className="startup-guide startup-guide--error">
            <strong>自动安装失败</strong>
            <span>{runtimeInstallError}</span>
          </div>
        )}

        <div className="startup-actions">
          <KtButton variant="primary" onClick={onInstallRuntime} loading={installingRuntime}>
            <RefreshCw size={16} />
            自动安装/修复
          </KtButton>
          <KtButton variant="default" onClick={onRetry}>
            <RefreshCw size={16} />
            重新检测
          </KtButton>
        </div>
      </div>
    </div>
  )
}

function RuntimeInstallGuide({ guide }: { guide: BrowserRuntimeInstallGuide }) {
  return (
    <div className="startup-guide">
      <strong>{guide.runtime} 安装引导</strong>
      <ol>
        {guide.steps.map((step) => (
          <li key={step}>{step}</li>
        ))}
      </ol>
      {guide.notes.length > 0 && <span>{guide.notes[0]}</span>}
    </div>
  )
}
