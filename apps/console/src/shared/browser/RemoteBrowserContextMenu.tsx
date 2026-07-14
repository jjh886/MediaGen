import {
  ArrowLeft,
  ArrowRight,
  Clipboard,
  Copy,
  RefreshCw,
  Scissors,
  TextSelect,
} from 'lucide-react'

export interface ContextMenuPosition {
  x: number
  y: number
}

type ContextAction = 'back' | 'forward' | 'reload' | 'copy' | 'cut' | 'paste' | 'select_all'

interface RemoteBrowserContextMenuProps {
  position: ContextMenuPosition
  canCopy: boolean
  canEdit: boolean
  onAction: (action: ContextAction) => void
  onClose: () => void
}

const ITEMS = [
  ['back', '后退', ArrowLeft],
  ['forward', '前进', ArrowRight],
  ['reload', '刷新', RefreshCw],
  ['copy', '复制', Copy],
  ['cut', '剪切', Scissors],
  ['paste', '粘贴', Clipboard],
  ['select_all', '全选', TextSelect],
] as const

export default function RemoteBrowserContextMenu({
  position,
  canCopy,
  canEdit,
  onAction,
  onClose,
}: RemoteBrowserContextMenuProps) {
  return (
    <div
      role="menu"
      aria-label="网页菜单"
      onPointerDown={(event) => event.stopPropagation()}
      style={{
        position: 'fixed',
        left: position.x,
        top: position.y,
        zIndex: 1200,
        width: 184,
        padding: 6,
        border: '1px solid #c7c7c7',
        borderRadius: 6,
        // 远程网页菜单模拟浏览器原生浅色菜单，不能继承控制台的深色文字变量。
        background: '#fff',
        color: '#202124',
        boxShadow: '0 12px 32px rgba(0,0,0,0.22)',
      }}
    >
      {ITEMS.map(([action, label, Icon], index) => {
        const disabled =
          (action === 'copy' && !canCopy) || (['cut', 'paste'].includes(action) && !canEdit)
        return (
          <button
            key={action}
            type="button"
            role="menuitem"
            disabled={disabled}
            onClick={() => {
              onAction(action)
              onClose()
            }}
            style={{
              width: '100%',
              height: 32,
              display: 'flex',
              alignItems: 'center',
              gap: 10,
              marginTop: index === 3 ? 5 : 0,
              padding: '0 9px',
              border: 0,
              borderTop: index === 3 ? '1px solid #dadce0' : undefined,
              background: 'transparent',
              color: disabled ? '#9aa0a6' : '#202124',
              cursor: disabled ? 'not-allowed' : 'pointer',
              fontSize: 13,
              textAlign: 'left',
            }}
          >
            <Icon size={15} aria-hidden="true" />
            {label}
          </button>
        )
      })}
    </div>
  )
}
