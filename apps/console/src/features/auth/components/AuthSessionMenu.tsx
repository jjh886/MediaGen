import { LogOut } from 'lucide-react'
import { logoutAdmin } from '../../../shared/services/auth-service'

export function AuthSessionMenu() {
  const logout = async () => {
    await logoutAdmin().catch(() => undefined)
    window.location.reload()
  }

  return (
    <button className="sidebar__auth-logout" onClick={logout} type="button">
      <LogOut size={16} />
      <span>退出登录</span>
    </button>
  )
}
