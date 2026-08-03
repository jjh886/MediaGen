export interface WeixinModelView {
  modelId: string
  displayName: string
  platform: string
}

export interface WeixinSettingsView {
  status: string
  maskedUserId: string
  lastInboundAt?: string | null
  lastOutboundAt?: string | null
  generationEnabled: boolean
  directPromptEnabled: boolean
  defaultModelId: string
  defaultAspectRatio: string
  defaultMaxImages: number
  proactiveNotificationsEnabled: boolean
  notifyDesktopTasks: boolean
  notifyTaskSuccesses: boolean
  notifyNoAccounts: boolean
  notifyAccountActions: boolean
  quietHoursJson: string
  models: WeixinModelView[]
}

export interface WeixinSettingsUpdate {
  generationEnabled: boolean
  directPromptEnabled: boolean
  defaultModelId: string
  defaultAspectRatio: string
  defaultMaxImages: number
  proactiveNotificationsEnabled: boolean
  notifyDesktopTasks: boolean
  notifyTaskSuccesses: boolean
  notifyNoAccounts: boolean
  notifyAccountActions: boolean
  quietHoursJson: string
}

export interface BindingActionResult {
  status: string
  message: string
  sessionId?: string
  qrImage?: string
  activationCode?: string
}

export interface WeixinActionRequest {
  action: string
  sessionId?: string
  value?: string
  settings?: WeixinSettingsUpdate
}
