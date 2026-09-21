import { act, renderHook } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'
import { readPageRoute, usePageRoute } from './usePageRoute'
afterEach(() => window.history.replaceState(null, '', '/'))
describe('页面 URL 路由', () => {
  it('读取深链接、保存任务筛选，并响应前进后退', () => {
    window.history.replaceState(null, '', '/logs?task_id=task-a')
    const { result } = renderHook(() => usePageRoute())
    expect(result.current.route).toEqual({ page: 'logs', taskId: 'task-a' })
    act(() => result.current.navigate('proxies'))
    expect(window.location.pathname).toBe('/proxies')
    expect(result.current.route.taskId).toBeNull()
    act(() => {
      window.history.replaceState(null, '', '/verifications?task_id=task-b')
      window.dispatchEvent(new PopStateEvent('popstate'))
    })
    expect(result.current.route).toEqual({ page: 'verifications', taskId: 'task-b' })
  })
  it('根路径归一到账号页，未知地址不误显示设置页', () => {
    window.history.replaceState(null, '', '/')
    const { unmount } = renderHook(() => usePageRoute())
    expect(window.location.pathname).toBe('/accounts')
    unmount()
    window.history.replaceState(null, '', '/unknown-page')
    expect(readPageRoute().page).toBeNull()
  })
})
