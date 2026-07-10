import { useEffect, useMemo, useState } from 'react'
import { useAccountStore } from '../../features/accounts/hooks/useAccountStore'
import { useTaskStore } from '../../features/tasks/hooks/useTaskStore'
import { checkHealth, getSystemInfo, type SystemInfo } from '../../shared/services/system-service'

interface ServiceStatus {
  ok: boolean
  text: string
}

function modeText(info: SystemInfo | null) {
  if (!info) return '正在识别运行模式'
  return info.exit_supported ? '本机启动器' : '远程服务器'
}

export default function StatusBar() {
  const accounts = useAccountStore((s) => s.accounts)
  const fetchAccounts = useAccountStore((s) => s.fetch)
  const tasks = useTaskStore((s) => s.tasks)
  const fetchTasks = useTaskStore((s) => s.fetch)
  const [serviceStatus, setServiceStatus] = useState<ServiceStatus>({
    ok: false,
    text: '正在连接 API 服务',
  })
  const [systemInfo, setSystemInfo] = useState<SystemInfo | null>(null)

  useEffect(() => {
    void fetchAccounts('').catch(() => undefined)
    void fetchTasks('').catch(() => undefined)
  }, [fetchAccounts, fetchTasks])

  useEffect(() => {
    let cancelled = false
    const refresh = async () => {
      try {
        const [health, info] = await Promise.all([checkHealth(), getSystemInfo()])
        if (!cancelled) {
          setServiceStatus({ ok: health.ok, text: health.ok ? 'API 服务运行中' : 'API 服务异常' })
          setSystemInfo(info)
        }
      } catch {
        if (!cancelled) {
          setServiceStatus({ ok: false, text: 'API 服务连接失败' })
          setSystemInfo(null)
        }
      }
    }
    void refresh()
    const timer = window.setInterval(refresh, 5000)
    return () => {
      cancelled = true
      window.clearInterval(timer)
    }
  }, [])

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
        <span
          className={`status-bar__indicator ${
            serviceStatus.ok ? 'status-bar__indicator--success' : 'status-bar__indicator--error'
          }`}
        />
        <span>{serviceStatus.text}</span>
        <span>{modeText(systemInfo)}</span>
      </div>
      <div className="status-bar__right">
        <span>可用账号: {availableCount}</span>
        <span>队列: {queueCount}</span>
      </div>
    </footer>
  )
}
