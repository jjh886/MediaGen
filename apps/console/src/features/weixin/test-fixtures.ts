import type { WeixinSettingsUpdate, WeixinSettingsView } from './types/weixin'

export const weixinSettingsUpdateFixture: WeixinSettingsUpdate = {
  generationEnabled: true,
  directPromptEnabled: false,
  defaultModelId: 'gpt-image-1',
  defaultAspectRatio: '9:16',
  defaultMaxImages: 1,
  proactiveNotificationsEnabled: true,
  notifyDesktopTasks: true,
  notifyTaskSuccesses: false,
  notifyNoAccounts: true,
  notifyAccountActions: true,
  quietHoursJson: '',
}

export const weixinSettingsFixture: WeixinSettingsView = {
  status: 'active',
  maskedUserId: 'user***0001',
  lastInboundAt: '2026-08-01T10:00:00Z',
  lastOutboundAt: null,
  ...weixinSettingsUpdateFixture,
  models: [{ modelId: 'gpt-image-1', displayName: 'GPT Image', platform: 'chatgpt' }],
}
