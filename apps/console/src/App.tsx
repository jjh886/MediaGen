import { useState } from 'react'
import { StartupGate } from './app/startup/StartupGate'
import AppShell from './app/layout/AppShell'

export type PageId = 'tasks' | 'accounts' | 'logs' | 'settings'

// 账号是使用 MediaGen 前最需要先确认的入口，所以应用首次打开默认进入账号页。
const DEFAULT_PAGE: PageId = 'accounts'

export default function App() {
  const [currentPage, setCurrentPage] = useState<PageId>(DEFAULT_PAGE)

  return (
    <StartupGate>
      <AppShell currentPage={currentPage} onNavigate={setCurrentPage} />
    </StartupGate>
  )
}
