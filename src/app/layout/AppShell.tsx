import type { PageId } from '../../App'
import Sidebar from './Sidebar'
import StatusBar from './StatusBar'
import TaskList from '../../features/tasks/components/TaskList'
import AccountPage from '../../features/accounts/components/AccountPage'
import { LogPage } from '../../features/logs/components/LogPage'
import { SettingsPage } from '../../features/settings/components/SettingsPage'

interface AppShellProps {
  currentPage: PageId
  onNavigate: (page: PageId) => void
}

export default function AppShell({ currentPage, onNavigate }: AppShellProps) {
  return (
    <div className="app-shell">
      <div className="app-shell__body">
        <Sidebar currentPage={currentPage} onNavigate={onNavigate} />
        <main className="app-shell__main">
          <PageContent page={currentPage} />
        </main>
      </div>
      <StatusBar />
    </div>
  )
}

function PageContent({ page }: { page: PageId }) {
  switch (page) {
    case 'tasks':
      return <TaskList />
    case 'accounts':
      return <AccountPage />
    case 'logs':
      return <LogPage />
    case 'settings':
      return <SettingsPage />
    default:
      return <PagePlaceholder page={page} />
  }
}

function PagePlaceholder({ page }: { page: PageId }) {
  const titles: Record<PageId, string> = {
    tasks: '任务',
    accounts: '账号',
    logs: '日志',
    settings: '设置',
  }
  return (
    <div className="page-placeholder">
      <h1>{titles[page]}</h1>
      <p style={{ color: 'var(--text-secondary)' }}>页面开发中...</p>
    </div>
  )
}
