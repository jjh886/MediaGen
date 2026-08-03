import { apiGet, apiPost } from '@/shared/services/http-api-client'
import type {
  BindingActionResult,
  WeixinActionRequest,
  WeixinSettingsUpdate,
  WeixinSettingsView,
} from '../types/weixin'

export async function fetchWeixinSettings(): Promise<WeixinSettingsView> {
  return await apiGet<WeixinSettingsView>('/api/weixin/settings')
}

export async function startWeixinBinding(): Promise<BindingActionResult> {
  return await apiPost<BindingActionResult>('/api/weixin/binding', {})
}

export async function runWeixinAction(request: WeixinActionRequest): Promise<BindingActionResult> {
  return await apiPost<BindingActionResult>('/api/weixin/actions', request)
}

export async function saveWeixinSettings(
  settings: WeixinSettingsUpdate
): Promise<BindingActionResult> {
  return await runWeixinAction({ action: 'save_settings', settings })
}
