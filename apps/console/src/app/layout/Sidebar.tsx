import { useEffect, useState } from 'react'
import {
  BookOpen,
  ChevronDown,
  FileText,
  KeyRound,
  Layers,
  ListOrdered,
  MessageCircle,
  PanelLeftClose,
  PanelLeftOpen,
  Settings,
  ShieldCheck,
  UserCog,
  Users,
  Zap,
} from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import type { PageId } from '../../App'
import { AuthSessionMenu } from '../../features/auth/components/AuthSessionMenu'
import { SystemControlMenu } from '../../features/settings/components/SystemControlCard'
import './sidebar.css'

interface SidebarProps {
  currentPage: PageId
  onNavigate: (page: PageId) => void
  adminEnabled: boolean
}
interface MenuGroup {
  title: string
  items: { id: PageId; label: string; icon: LucideIcon; adminOnly?: boolean }[]
}
const GROUPS: MenuGroup[] = [
  {
    title: '工作台',
    items: [
      { id: 'tasks', label: '任务', icon: Zap },
      { id: 'generation-queue', label: '生成队列', icon: ListOrdered },
      { id: 'verifications', label: '人机接管', icon: Layers },
    ],
  },
  {
    title: '资源配置',
    items: [
      { id: 'accounts', label: '账号', icon: Users },
      { id: 'proxies', label: '代理', icon: ShieldCheck },
    ],
  },
  {
    title: '开发接入',
    items: [
      { id: 'api-keys', label: 'API Key', icon: KeyRound },
      { id: 'api-docs', label: '接口文档', icon: BookOpen },
      { id: 'weixin', label: '微信', icon: MessageCircle },
    ],
  },
  {
    title: '系统管理',
    items: [
      { id: 'logs', label: '日志', icon: FileText },
      { id: 'admin-users', label: '后台用户', icon: UserCog, adminOnly: true },
      { id: 'settings', label: '设置', icon: Settings },
    ],
  },
]

export default function Sidebar({ currentPage, onNavigate, adminEnabled }: SidebarProps) {
  const [compact, setCompact] = useState(false)
  const [closed, setClosed] = useState<Record<string, boolean>>({})
  useEffect(() => {
    const group = GROUPS.find((group) => group.items.some((item) => item.id === currentPage))
    if (group) setClosed((previous) => ({ ...previous, [group.title]: false }))
  }, [currentPage])
  return (
    <aside className={`sidebar${compact ? ' sidebar--compact' : ''}`}>
      <div className="sidebar__brand">
        <span className="sidebar__mark" aria-hidden="true">
          <Layers size={21} />
        </span>
        <div className="sidebar__brand-copy">
          <strong>MediaGen</strong>
          <span>工作空间</span>
        </div>
        <button
          type="button"
          className="sidebar__toggle"
          aria-label={compact ? '展开菜单' : '收起菜单'}
          title={compact ? '展开菜单' : '收起菜单'}
          aria-expanded={!compact}
          onClick={() => setCompact(!compact)}
        >
          {compact ? <PanelLeftOpen size={18} /> : <PanelLeftClose size={18} />}
        </button>
      </div>
      <nav className="sidebar__nav" aria-label="主导航">
        {GROUPS.map((group, index) => {
          const items = group.items.filter((item) => !item.adminOnly || adminEnabled)
          const active = items.some((item) => item.id === currentPage)
          return (
            <section className="sidebar__group" key={group.title} aria-label={group.title}>
              <button
                type="button"
                className={`sidebar__group-heading${active ? ' is-active' : ''}`}
                aria-expanded={!closed[group.title]}
                aria-controls={`menu-group-${index}`}
                onClick={() =>
                  setClosed((previous) => ({ ...previous, [group.title]: !previous[group.title] }))
                }
              >
                <span>{group.title}</span>
                <ChevronDown size={13} />
              </button>
              <div
                id={`menu-group-${index}`}
                className="sidebar__group-items"
                hidden={!compact && closed[group.title]}
              >
                {items.map(({ id, label, icon: Icon }) => (
                  <a
                    key={id}
                    href={`/${id}`}
                    title={compact ? label : undefined}
                    aria-label={label}
                    aria-current={currentPage === id ? 'page' : undefined}
                    className={`sidebar__item${currentPage === id ? ' sidebar__item--active' : ''}`}
                    onClick={(event) => {
                      if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return
                      event.preventDefault()
                      onNavigate(id)
                    }}
                  >
                    <Icon size={18} className="sidebar__item-icon" />
                    <span className="sidebar__item-label">{label}</span>
                  </a>
                ))}
              </div>
            </section>
          )
        })}
      </nav>
      <div className="sidebar__footer">
        {adminEnabled && <AuthSessionMenu />}
        <SystemControlMenu />
      </div>
    </aside>
  )
}
