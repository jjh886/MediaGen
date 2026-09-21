import { useEffect } from 'react'
import type { PageId } from '../../App'
import Sidebar from './Sidebar'
import AdminUsersPage from '../../features/admin/components/AdminUsersPage'
import StatusBar from './StatusBar'
import TaskList from '../../features/tasks/components/TaskList'
import GenerationQueueList from '../../features/tasks/components/GenerationQueueList'
import AccountPage from '../../features/accounts/components/AccountPage'
import ApiDocsPage from '../../features/api-docs/ApiDocsPage'
import WeixinPage from '../../features/weixin/components/WeixinPage'
import { LogPage } from '../../features/logs/components/LogPage'
import { SettingsPage } from '../../features/settings/components/SettingsPage'
import {
  ApiKeysPage,
  ProxiesPage,
  VerificationSessionsPage,
} from '../../features/admin/components/AdminPages'

interface AppShellProps {
  adminEnabled?: boolean
  currentPage: PageId
  verificationTaskId: string | null
  logTaskId: string | null
  onNavigate: (page: PageId) => void
  onOpenVerification: (taskId: string) => void
  onOpenTaskLogs: (taskId: string) => void
}

export default function AppShell(props: AppShellProps) {
  useEffect(() => {
    if (props.currentPage === 'admin-users' && !props.adminEnabled) props.onNavigate('accounts')
  }, [props.currentPage, props.adminEnabled, props.onNavigate])
  return (
    <div className="app-shell">
      <div className="app-shell__body">
        <Sidebar
          currentPage={props.currentPage}
          onNavigate={props.onNavigate}
          adminEnabled={!!props.adminEnabled}
        />
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
  if (currentPage === 'generation-queue') return <GenerationQueueList />
  if (currentPage === 'weixin') return <WeixinPage />
  if (currentPage === 'accounts') return <AccountPage />
  if (currentPage === 'admin-users')
    return props.adminEnabled ? <AdminUsersPage /> : <AccountPage />
  if (currentPage === 'api-docs') return <ApiDocsPage />
  if (currentPage === 'api-keys') return <ApiKeysPage />
  if (currentPage === 'proxies') return <ProxiesPage />
  if (currentPage === 'verifications') {
    return <VerificationSessionsPage targetTaskId={verificationTaskId} />
  }
  if (currentPage === 'logs') return <LogPage initialTaskId={logTaskId} />
  return <SettingsPage />
}
