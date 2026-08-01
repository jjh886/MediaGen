import { useEffect, useState } from 'react'
import { LogOut } from 'lucide-react'
import { getAuthStatus, logoutAdmin } from '../../../shared/services/auth-service'

export function AuthSessionMenu() {
  const [authConfigured, setAuthConfigured] = useState(false)

  useEffect(() => {
    let cancelled = false
    getAuthStatus()
      .then((status) => {
        if (!cancelled) setAuthConfigured(status.supported && status.configured)
      })
      .catch(() => {
        // 无法确认服务器登录配置时保持隐藏，避免本机应用出现无效的退出入口。
      })
    return () => {
      cancelled = true
    }
  }, [])

  const logout = async () => {
    await logoutAdmin().catch(() => undefined)
    window.location.reload()
  }

  if (!authConfigured) return null

  return (
    <button className="sidebar__auth-logout" onClick={logout} type="button">
      <LogOut size={16} />
      <span>退出登录</span>
    </button>
  )
}
