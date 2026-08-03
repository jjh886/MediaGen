import { BellRing } from 'lucide-react'
import type { WeixinSettingsUpdate } from '../types/weixin'
import { ToggleRow, WeixinPanel } from './WeixinShared'

interface WeixinNotificationPanelProps {
  draft: WeixinSettingsUpdate
  onChange: (patch: Partial<WeixinSettingsUpdate>) => void
}

export function WeixinNotificationPanel({ draft, onChange }: WeixinNotificationPanelProps) {
  return (
    <WeixinPanel title="微信通知">
      <div
        style={{ display: 'flex', alignItems: 'center', gap: 10, color: 'var(--text-secondary)' }}
      >
        <BellRing size={18} />
        <span style={{ fontSize: 13 }}>
          通知会通过已绑定微信发送，适合长时间排队或账号异常时提醒。
        </span>
      </div>
      <ToggleRow
        title="主动通知总开关"
        description="关闭后只回复微信指令，不主动推送本机任务或账号变化。"
        checked={draft.proactiveNotificationsEnabled}
        onChange={(proactiveNotificationsEnabled) => onChange({ proactiveNotificationsEnabled })}
      />
      <ToggleRow
        title="任务失败通知"
        description="生成失败或需要处理时发送提醒，方便及时接管。"
        checked={draft.notifyDesktopTasks}
        onChange={(notifyDesktopTasks) => onChange({ notifyDesktopTasks })}
      />
      <ToggleRow
        title="任务成功通知"
        description="任务完成时也推送一条状态消息；微信发起的生图结果始终会回传。"
        checked={draft.notifyTaskSuccesses}
        onChange={(notifyTaskSuccesses) => onChange({ notifyTaskSuccesses })}
      />
      <ToggleRow
        title="无可用账号通知"
        description="所有账号都不可用且没有明确接管项时，提醒你去账号页查看。"
        checked={draft.notifyNoAccounts}
        onChange={(notifyNoAccounts) => onChange({ notifyNoAccounts })}
      />
      <ToggleRow
        title="账号需要处理通知"
        description="登录过期、需要人机验证或总额度耗尽时发送提醒。"
        checked={draft.notifyAccountActions}
        onChange={(notifyAccountActions) => onChange({ notifyAccountActions })}
      />
    </WeixinPanel>
  )
}
