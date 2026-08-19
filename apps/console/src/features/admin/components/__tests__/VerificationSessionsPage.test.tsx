import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import VerificationSessionsPage from '../VerificationSessionsPage'

const mocks = vi.hoisted(() => ({
  fetchBrowserSessions: vi.fn(),
  fetchBrowserSession: vi.fn(),
  completeBrowserSession: vi.fn(),
  cancelBrowserSession: vi.fn(),
  browserSessionHistory: vi.fn(),
}))

vi.mock('../../services/admin-service', () => ({
  fetchBrowserSessions: (...args: unknown[]) => mocks.fetchBrowserSessions(...args),
}))

vi.mock('@/shared/services/browser-session-service', () => ({
  fetchBrowserSession: (...args: unknown[]) => mocks.fetchBrowserSession(...args),
  completeBrowserSession: (...args: unknown[]) => mocks.completeBrowserSession(...args),
  cancelBrowserSession: (...args: unknown[]) => mocks.cancelBrowserSession(...args),
  browserSessionHistory: (...args: unknown[]) => mocks.browserSessionHistory(...args),
}))

vi.mock('@/shared/browser/RemoteBrowserPanel', () => ({
  default: () => <div>远程浏览器画面</div>,
}))

describe('VerificationSessionsPage', () => {
  beforeEach(() => {
    mocks.fetchBrowserSessions.mockReset()
    mocks.fetchBrowserSession.mockReset()
    mocks.completeBrowserSession.mockReset()
    mocks.cancelBrowserSession.mockReset()
    mocks.browserSessionHistory.mockReset()
    mocks.fetchBrowserSessions.mockResolvedValue([
      {
        id: 'session-1',
        purpose: 'human_verification',
        status: 'active',
        platform: 'chatgpt',
        account_id: 'acc-1',
        task_id: null,
        url: 'https://chatgpt.com',
        title: '',
        created_at: '2026-06-29T00:00:00Z',
      },
      {
        id: 'session-2',
        purpose: 'debug_preview',
        status: 'active',
        platform: 'codex',
        account_id: null,
        task_id: null,
        url: 'about:blank',
        title: '',
        created_at: '2026-06-29T00:00:00Z',
      },
    ])
    mocks.fetchBrowserSession.mockResolvedValue({
      id: 'session-1',
      purpose: 'human_verification',
      status: 'active',
      platform: 'chatgpt',
      account_id: 'acc-1',
      task_id: null,
      start_url: 'https://chatgpt.com',
      url: 'https://chatgpt.com',
      title: '',
      target_id: 'target-1',
      headless: true,
      browser_name: 'CloakBrowser',
      runtime_id: 'cloakbrowser',
      viewport: { width: 1365, height: 768 },
      device_scale_factor: 1,
      message: '',
      cookie_count: 0,
      profile_dir: '/tmp/profile',
      created_at: '2026-06-29T00:00:00Z',
      updated_at: '2026-06-29T00:00:00Z',
    })
  })

  it('只展示需要接管的会话，并能打开远程浏览器窗口', async () => {
    render(<VerificationSessionsPage />)

    expect(await screen.findByText('chatgpt')).toBeInTheDocument()
    expect(mocks.fetchBrowserSessions).toHaveBeenCalledWith(40, 0, '')
    expect(screen.queryByText('codex')).not.toBeInTheDocument()
    fireEvent.click(screen.getByText('打开接管'))

    await waitFor(() => {
      expect(mocks.fetchBrowserSession).toHaveBeenCalledWith('session-1')
    })
    expect(screen.getByText('远程浏览器画面')).toBeInTheDocument()
  })
})
