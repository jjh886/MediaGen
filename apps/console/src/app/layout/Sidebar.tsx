import {
  FileText,
  BookOpen,
  KeyRound,
  Layers,
  ListOrdered,
  MessageCircle,
  Settings as SettingsIcon,
  ShieldCheck,
  Users,
  Zap,
} from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import type { PageId } from '../../App'
import { AuthSessionMenu } from '../../features/auth/components/AuthSessionMenu'
import { SystemControlMenu } from '../../features/settings/components/SystemControlCard'

interface SidebarProps {
  currentPage: PageId
  onNavigate: (page: PageId) => void
}

const NAV_ITEMS: { id: PageId; label: string; icon: LucideIcon }[] = [
  { id: 'accounts', label: '账号', icon: Users },
  { id: 'api-keys', label: 'API Key', icon: KeyRound },
  { id: 'api-docs', label: '接口文档', icon: BookOpen },
  { id: 'tasks', label: '任务', icon: Zap },
  { id: 'generation-queue', label: '生成队列', icon: ListOrdered },
  { id: 'weixin', label: '微信', icon: MessageCircle },
  { id: 'proxies', label: '代理', icon: ShieldCheck },
  { id: 'verifications', label: '人机接管', icon: Layers },
  { id: 'logs', label: '日志', icon: FileText },
  { id: 'admin-users', label: '后台用户', icon: Users },
  { id: 'settings', label: '设置', icon: SettingsIcon },
]

export default function Sidebar({ currentPage, onNavigate }: SidebarProps) {
  return (
    <aside className="sidebar">
      <div className="sidebar__logo">
        <span className="sidebar__logo-text">MediaGen Studio</span>
      </div>
      <nav className="sidebar__nav">
        {NAV_ITEMS.map((item) => {
          const Icon = item.icon
          return (
            <button
              key={item.id}
              className={`sidebar__item ${currentPage === item.id ? 'sidebar__item--active' : ''}`}
              onClick={() => onNavigate(item.id)}
            >
              <Icon size={20} className="sidebar__item-icon" />
              <span className="sidebar__item-label">{item.label}</span>
            </button>
          )
        })}
      </nav>
      <AuthSessionMenu />
      <SystemControlMenu />
    </aside>
  )
}
