import { useEffect, useState } from 'react'
import { fetchGenerationQueue } from '../../features/tasks/services/generation-queue-service'
import { fetchAccountSummary } from '../../shared/services/account-service'
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
  const [queueCounts, setQueueCounts] = useState({ image: 0, text: 0 })
  const [availableCount, setAvailableCount] = useState(0)
  const [serviceStatus, setServiceStatus] = useState<ServiceStatus>({
    ok: false,
    text: '正在连接 API 服务',
  })
  const [systemInfo, setSystemInfo] = useState<SystemInfo | null>(null)

  useEffect(() => {
    const refreshCounts = () => {
      void fetchAccountSummary()
        .then((summary) => setAvailableCount(summary.available))
        .catch(() => undefined)
      void Promise.all([fetchGenerationQueue(1, 0, 'image'), fetchGenerationQueue(1, 0, 'text')])
        .then(([image, text]) => setQueueCounts({ image: image.total, text: text.total }))
        .catch(() => undefined)
    }
    refreshCounts()
    const timer = window.setInterval(refreshCounts, 5000)
    return () => window.clearInterval(timer)
  }, [])

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
        <span>图片排队: {queueCounts.image}</span>
        <span>文本排队: {queueCounts.text}</span>
      </div>
    </footer>
  )
}
