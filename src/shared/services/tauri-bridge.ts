/**
 * Tauri Command 统一封装层。
 * 所有 feature 的 service 通过此模块调用 Rust 端，组件层禁止直接 import @tauri-apps/api。
 */

// 开发环境（纯 Vite）下 @tauri-apps/api 不可用，提供 mock fallback
const isTauri = '__TAURI_INTERNALS__' in window

async function tauriInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri) {
    console.warn(`[tauri-bridge] Tauri 不可用，跳过命令: ${cmd}`)
    return undefined as T
  }
  const { invoke } = await import('@tauri-apps/api/core')
  return invoke<T>(cmd, args)
}

export { tauriInvoke }
