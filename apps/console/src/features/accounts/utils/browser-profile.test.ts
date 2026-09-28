import { describe, expect, it } from 'vitest'
import {
  buildBrowserProfilePayload,
  browserVersionChoices,
  parseBrowserProfile,
  type BrowserOs,
} from './browser-profile'

describe('browser-profile', () => {
  it.each<BrowserOs>(['windows', 'macos', 'linux'])('为 %s 使用选中的版本', (os) => {
    expect(buildBrowserProfilePayload({ enabled: true, os, version: '146' })).toEqual({
      browser_os: os,
      browser_version: '146',
    })
  })

  it('读取旧账号时保留已选版本，并按最新稳定版生成候选项', () => {
    expect(parseBrowserProfile('macos', '128')).toEqual({
      enabled: true,
      os: 'macos',
      version: '128',
    })
    expect(browserVersionChoices('154')).toEqual(['154', '153', '152'])
  })

  it('关闭模拟后不写入系统和版本', () => {
    expect(buildBrowserProfilePayload({ enabled: false, os: 'windows', version: '146' })).toEqual({
      browser_os: '',
      browser_version: '',
    })
  })
})
