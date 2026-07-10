import { useState } from 'react'
import AppShell from './app/layout/AppShell'

export type PageId =
  | 'accounts'
  | 'api-keys'
  | 'tasks'
  | 'proxies'
  | 'verifications'
  | 'logs'
  | 'settings'

// 账号是使用 MediaGen 前最需要先确认的入口，所以应用首次打开默认进入账号页。
const DEFAULT_PAGE: PageId = 'accounts'

export default function App() {
  const [currentPage, setCurrentPage] = useState<PageId>(DEFAULT_PAGE)
  const [verificationTaskId, setVerificationTaskId] = useState<string | null>(null)
  const [logTaskId, setLogTaskId] = useState<string | null>(null)

  const openVerification = (taskId: string) => {
    setVerificationTaskId(taskId)
    setCurrentPage('verifications')
  }
  const openTaskLogs = (taskId: string) => {
    setLogTaskId(taskId)
    setCurrentPage('logs')
  }

  return (
    <AppShell
      currentPage={currentPage}
      verificationTaskId={verificationTaskId}
      logTaskId={logTaskId}
      onNavigate={setCurrentPage}
      onOpenVerification={openVerification}
      onOpenTaskLogs={openTaskLogs}
    />
  )
}
