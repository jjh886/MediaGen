import { Zap, Users, FileText, Settings as SettingsIcon } from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import type { PageId } from '../../App'

interface SidebarProps {
  currentPage: PageId
  onNavigate: (page: PageId) => void
}

const NAV_ITEMS: { id: PageId; label: string; icon: LucideIcon }[] = [
  { id: 'accounts', label: '账号', icon: Users },
  { id: 'tasks', label: '任务', icon: Zap },
  { id: 'logs', label: '日志', icon: FileText },
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
    </aside>
  )
}
