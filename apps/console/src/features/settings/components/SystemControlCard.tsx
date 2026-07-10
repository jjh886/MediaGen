import { useEffect, useState } from 'react'
import { Power } from 'lucide-react'
import { KtButton } from '@/shared/ui'
import { exitMediaGen, getSystemInfo } from '@/shared/services/system-service'

export function SystemControlMenu() {
  const [exitSupported, setExitSupported] = useState(false)
  const [loaded, setLoaded] = useState(false)
  const [exiting, setExiting] = useState(false)
  const [message, setMessage] = useState('')

  useEffect(() => {
    let alive = true
    // 只在桌面启动器支持安全退出时展示入口，避免普通网页环境误导用户。
    getSystemInfo()
      .then((info) => {
        if (alive) setExitSupported(info.exit_supported)
      })
      .catch(() => {
        if (alive) setExitSupported(false)
      })
      .finally(() => {
        if (alive) setLoaded(true)
      })
    return () => {
      alive = false
    }
  }, [])

  async function handleExit() {
    const confirmed = window.confirm('退出后会停止后台服务，正在运行的任务会中断。确认退出吗？')
    if (!confirmed) return

    setExiting(true)
    setMessage('')
    try {
      await exitMediaGen()
      setMessage('MediaGen 正在退出，可以关闭这个页面。')
    } catch (error) {
      setMessage(error instanceof Error ? error.message : '退出失败，请稍后再试')
      setExiting(false)
    }
  }

  if (!loaded || !exitSupported) return null

  return (
    <div className="sidebar__system-control">
      <div className="sidebar__system-title">应用控制</div>
      <p className="sidebar__system-text">退出会停止本机后台服务。</p>
      <KtButton
        variant="danger"
        size="sm"
        loading={exiting}
        onClick={handleExit}
        style={{ width: '100%' }}
      >
        <Power size={15} />
        退出 MediaGen
      </KtButton>
      {message && <div className="sidebar__system-message">{message}</div>}
    </div>
  )
}
