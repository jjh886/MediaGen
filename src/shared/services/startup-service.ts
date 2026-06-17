import { tauriInvoke } from '@/shared/services/tauri-bridge'

export interface MacosPermissionItem {
  key: string
  title: string
  granted: boolean
  blocking: boolean
  message: string
}

export interface MacosStartupPermissionStatus {
  required: boolean
  granted: boolean
  permissions: MacosPermissionItem[]
}

export type StartupBlockReason = 'chrome' | 'macos-permission'

export interface StartupReadiness {
  ok: boolean
  reason?: StartupBlockReason
  title?: string
  message?: string
  permissions?: MacosPermissionItem[]
}

const isTauri = '__TAURI_INTERNALS__' in window
const MACOS_APP_MANAGEMENT_CONFIRMED = 'startup:macos_app_management_confirmed'

/** 启动门禁：macOS / Windows 都先确认 Chrome；macOS 再额外确认系统授权。 */
export async function checkStartupReadiness(): Promise<StartupReadiness> {
  if (!isTauri) return { ok: true }

  const chrome = await checkChrome()
  if (!chrome.ok) return chrome

  const permissions = await checkMacosStartupPermissions(false)
  if (shouldBlockForMacosPermissions(permissions)) {
    return {
      ok: false,
      reason: 'macos-permission',
      title: '需要先开启 macOS App 管理权限',
      message:
        'MediaGen 在 macOS 上启动浏览器组件时可能触发 App 管理拦截。请先在系统设置里允许 MediaGen 管理应用，重启后再进入主应用。',
      permissions: permissions.permissions,
    }
  }

  return { ok: true }
}

export async function requestMacosStartupPermissions(): Promise<MacosStartupPermissionStatus> {
  return checkMacosStartupPermissions(true)
}

/** App Management 没有公开无侵入查询 API，只能由用户完成系统授权后确认。 */
export function confirmMacosAppManagementPermission(): void {
  localStorage.setItem(MACOS_APP_MANAGEMENT_CONFIRMED, 'true')
}

export async function openMacosPrivacySettings(): Promise<void> {
  if (!isTauri) return
  await tauriInvoke('open_macos_privacy_settings')
}

async function checkChrome(): Promise<StartupReadiness> {
  try {
    // Rust 端会按当前系统查找 Chrome：macOS 查 /Applications，Windows 查常见安装目录和 LOCALAPPDATA。
    await tauriInvoke<string>('check_chrome_available')
    return { ok: true }
  } catch (e) {
    return {
      ok: false,
      reason: 'chrome',
      title: '未检测到 Chrome 浏览器',
      message: String(e),
    }
  }
}

function shouldBlockForMacosPermissions(permissions: MacosStartupPermissionStatus): boolean {
  if (!permissions.required || permissions.granted) return false
  const appManagementPending = permissions.permissions.some(
    (item) => item.key === 'app_management' && item.blocking && !item.granted
  )
  if (!appManagementPending) return true
  return localStorage.getItem(MACOS_APP_MANAGEMENT_CONFIRMED) !== 'true'
}

async function checkMacosStartupPermissions(
  prompt: boolean
): Promise<MacosStartupPermissionStatus> {
  try {
    return await tauriInvoke<MacosStartupPermissionStatus>('check_macos_startup_permissions', {
      prompt,
    })
  } catch {
    // 非 macOS 或旧版本后端没有该命令时，不能误拦用户进入应用。
    return { required: false, granted: true, permissions: [] }
  }
}
