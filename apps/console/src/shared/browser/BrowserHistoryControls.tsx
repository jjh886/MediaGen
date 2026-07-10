import { ArrowLeft, ArrowRight, RotateCw } from 'lucide-react'
import BrowserToolbarIconButton from './BrowserToolbarIconButton'

type BrowserHistoryAction = 'back' | 'forward' | 'reload'

interface BrowserHistoryControlsProps {
  onAction: (action: BrowserHistoryAction) => void
}

// 统一远程浏览器的后退、前进、刷新控件，让登录弹窗和接管弹窗保持同一套交互。
export default function BrowserHistoryControls({ onAction }: BrowserHistoryControlsProps) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
      <BrowserToolbarIconButton label="后退" onClick={() => onAction('back')}>
        <ArrowLeft size={16} />
      </BrowserToolbarIconButton>
      <BrowserToolbarIconButton label="前进" onClick={() => onAction('forward')}>
        <ArrowRight size={16} />
      </BrowserToolbarIconButton>
      <BrowserToolbarIconButton label="刷新" onClick={() => onAction('reload')}>
        <RotateCw size={16} />
      </BrowserToolbarIconButton>
    </div>
  )
}
