import { describe, expect, it } from 'vitest'
import {
  buildBrowserProfilePayload,
  getDefaultChromeMajorVersion,
  parseBrowserProfile,
  type BrowserOs,
} from './browser-profile'

describe('browser-profile', () => {
  it.each<BrowserOs>(['windows', 'macos', 'linux'])('为 %s 自动使用当前稳定版本', (os) => {
    expect(getDefaultChromeMajorVersion(os)).toBe('150')
    expect(buildBrowserProfilePayload({ enabled: true, os })).toEqual({
      browser_os: os,
      browser_version: '150',
    })
  })

  it('读取旧账号时保留开启状态，但不再让旧版本控制新配置', () => {
    expect(parseBrowserProfile('macos', '128')).toEqual({ enabled: true, os: 'macos' })
  })

  it('关闭模拟后不写入系统和版本', () => {
    expect(buildBrowserProfilePayload({ enabled: false, os: 'windows' })).toEqual({
      browser_os: '',
      browser_version: '',
    })
  })
})
