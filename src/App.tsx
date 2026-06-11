import { useState, useEffect } from 'react'
import AppShell from './app/layout/AppShell'
import { checkChromeAvailable } from './shared/services/settings-service'

export type PageId = 'tasks' | 'accounts' | 'logs' | 'settings'

// 账号是使用 MediaGen 前最需要先确认的入口，所以应用首次打开默认进入账号页。
const DEFAULT_PAGE: PageId = 'accounts'

export default function App() {
  const [currentPage, setCurrentPage] = useState<PageId>(DEFAULT_PAGE)
  const [chromeError, setChromeError] = useState<string | null>(null)
  const [checking, setChecking] = useState(true)

  useEffect(() => {
    checkChromeAvailable().then((result) => {
      if (!result.ok) setChromeError(result.message)
      setChecking(false)
    })
  }, [])

  if (checking) return null

  if (chromeError) {
    return <ChromeErrorScreen message={chromeError} />
  }

  return <AppShell currentPage={currentPage} onNavigate={setCurrentPage} />
}

function ChromeErrorScreen({ message }: { message: string }) {
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        height: '100vh',
        backgroundColor: 'var(--bg-primary, #1a1a2e)',
        color: 'var(--text-primary, #e0e0e0)',
        padding: '40px',
        textAlign: 'center',
      }}
    >
      <div
        style={{
          fontSize: '48px',
          marginBottom: '24px',
          filter: 'grayscale(1)',
        }}
      >
        🌐
      </div>
      <h1 style={{ fontSize: '22px', marginBottom: '12px', fontWeight: 600 }}>
        未检测到 Chrome 浏览器
      </h1>
      <p
        style={{
          color: 'var(--text-secondary, #999)',
          fontSize: '14px',
          lineHeight: 1.8,
          maxWidth: '480px',
          whiteSpace: 'pre-line',
        }}
      >
        {message}
      </p>
      <p
        style={{
          color: 'var(--text-secondary, #999)',
          fontSize: '13px',
          marginTop: '24px',
        }}
      >
        安装完成后请重启应用
      </p>
    </div>
  )
}
