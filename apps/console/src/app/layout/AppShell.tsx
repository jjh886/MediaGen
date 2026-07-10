import type { PageId } from '../../App'
import Sidebar from './Sidebar'
import StatusBar from './StatusBar'
import TaskList from '../../features/tasks/components/TaskList'
import AccountPage from '../../features/accounts/components/AccountPage'
import { LogPage } from '../../features/logs/components/LogPage'
import { SettingsPage } from '../../features/settings/components/SettingsPage'
import {
  ApiKeysPage,
  ProxiesPage,
  VerificationSessionsPage,
} from '../../features/admin/components/AdminPages'

interface AppShellProps {
  currentPage: PageId
  verificationTaskId: string | null
  logTaskId: string | null
  onNavigate: (page: PageId) => void
  onOpenVerification: (taskId: string) => void
  onOpenTaskLogs: (taskId: string) => void
}

export default function AppShell(props: AppShellProps) {
  return (
    <div className="app-shell">
      <div className="app-shell__body">
        <Sidebar currentPage={props.currentPage} onNavigate={props.onNavigate} />
        <main className="app-shell__main">
          <PageContent {...props} />
        </main>
      </div>
      <StatusBar />
    </div>
  )
}

function PageContent(props: AppShellProps) {
  const { currentPage, verificationTaskId, logTaskId, onOpenVerification, onOpenTaskLogs } = props
  if (currentPage === 'tasks') {
    return <TaskList onOpenVerification={onOpenVerification} onOpenTaskLogs={onOpenTaskLogs} />
  }
  if (currentPage === 'accounts') return <AccountPage />
  if (currentPage === 'api-keys') return <ApiKeysPage />
  if (currentPage === 'proxies') return <ProxiesPage />
  if (currentPage === 'verifications') {
    return <VerificationSessionsPage targetTaskId={verificationTaskId} />
  }
  if (currentPage === 'logs') return <LogPage initialTaskId={logTaskId} />
  return <SettingsPage />
}
