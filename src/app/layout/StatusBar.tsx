import { useEffect, useMemo } from 'react'
import { useAccountStore } from '../../features/accounts/hooks/useAccountStore'
import { useTaskStore } from '../../features/tasks/hooks/useTaskStore'

export default function StatusBar() {
  const accounts = useAccountStore((s) => s.accounts)
  const fetchAccounts = useAccountStore((s) => s.fetch)
  const tasks = useTaskStore((s) => s.tasks)
  const fetchTasks = useTaskStore((s) => s.fetch)

  useEffect(() => {
    fetchAccounts('')
    fetchTasks('')
  }, [fetchAccounts, fetchTasks])

  const availableCount = useMemo(
    () => accounts.filter((a) => a.login_state === 'available').length,
    [accounts]
  )
  const queueCount = useMemo(
    () => tasks.filter((t) => t.status === 'pending' || t.status === 'processing').length,
    [tasks]
  )

  return (
    <footer className="status-bar">
      <div className="status-bar__left">
        <span className="status-bar__indicator status-bar__indicator--success" />
        <span>API 服务运行中</span>
      </div>
      <div className="status-bar__right">
        <span>可用账号: {availableCount}</span>
        <span>队列: {queueCount}</span>
      </div>
    </footer>
  )
}
