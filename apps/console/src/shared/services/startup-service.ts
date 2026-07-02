import {
  checkBrowserRuntimeAvailable,
  getBrowserRuntimeInstallGuide,
  installBrowserRuntime,
  type BrowserRuntimeInstallGuide,
} from '@/shared/services/settings-service'

export { getBrowserRuntimeInstallGuide, installBrowserRuntime }
export type { BrowserRuntimeInstallGuide }

export type StartupBlockReason = 'browser-runtime'

export interface StartupReadiness {
  ok: boolean
  reason?: StartupBlockReason
  title?: string
  message?: string
}

/** Web Console 启动前只确认服务端可用的 CloakBrowser 运行时。 */
export async function checkStartupReadiness(): Promise<StartupReadiness> {
  const runtime = await checkBrowserRuntimeAvailable()
  if (runtime.ok) return { ok: true }
  return {
    ok: false,
    reason: 'browser-runtime',
    title: '未检测到 CloakBrowser',
    message: runtime.message,
  }
}
