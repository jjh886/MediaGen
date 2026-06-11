import { beforeEach, describe, expect, it, vi } from 'vitest'

import {
  getCgptState,
  runScriptJson,
  setCgptState,
} from '../../../../test-utils/chatgpt/script-test-utils'

describe('chatgpt login and cleanup scripts', () => {
  beforeEach(() => {
    document.body.innerHTML = ''
    vi.useRealTimers()
    vi.unstubAllGlobals()
  })

  it('check_login accepts logged_in client bootstrap session by user id', async () => {
    document.body.innerHTML = `
      <script id="client-bootstrap" type="application/json">${JSON.stringify({
        authStatus: 'logged_in',
        session: {
          user: { id: 'user_current', name: 'Tester' },
          account: { planType: 'pro' },
        },
      })}</script>
    `

    const result = await runScriptJson('check_login.js')
    expect(result.loggedIn).toBe(true)
    expect(result.userId).toBe('user_current')
    expect(result.username).toBe('Tester')
    expect(result.email).toBe('')
    expect(result.tier).toBe('pro')
  })

  it('check_login ignores explicitly logged_out bootstrap data', async () => {
    document.body.innerHTML = `
      <script id="client-bootstrap" type="application/json">${JSON.stringify({
        authStatus: 'logged_out',
        session: {
          user: { id: 'user_stale', email: 'stale@example.com' },
        },
      })}</script>
    `
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('{}', { status: 401 }))
    )

    const result = await runScriptJson('check_login.js')
    expect(result.loggedIn).toBe(false)
  })

  it('delete_current_conversation hides the generated conversation and clears runtime ids', async () => {
    setCgptState({
      conversation_id: 'conv-current',
      result_conversation_id: 'conv-result',
    })
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({ success: true })))
    vi.stubGlobal('fetch', fetchMock)

    const result = await runScriptJson('delete_current_conversation.js')
    const [, init] = fetchMock.mock.calls[0]
    const state = getCgptState()

    expect(result.deleted).toBe(true)
    expect(fetchMock).toHaveBeenCalledWith(
      '/backend-api/conversation/conv-result',
      expect.objectContaining({
        method: 'PATCH',
        credentials: 'include',
        headers: { 'content-type': 'application/json' },
      })
    )
    expect(JSON.parse(String((init as RequestInit).body))).toEqual({ is_visible: false })
    expect(state.deleted_conversation_id).toBe('conv-result')
    expect(state.result_conversation_id).toBe('')
    expect(state.conversation_id).toBe('conv-current')
  })

  it('delete_current_conversation skips cleanup when conversation id is missing', async () => {
    setCgptState({})
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)

    const result = await runScriptJson('delete_current_conversation.js')

    expect(result.deleted).toBe(false)
    expect(result.skipped).toBe(true)
    expect(result.reason).toBe('missing_conversation_id')
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('delete_current_conversation reports delete failure without throwing', async () => {
    setCgptState({ conversation_id: 'conv-current' })
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async () =>
          new Response(JSON.stringify({ detail: 'permission denied' }), {
            status: 403,
            statusText: 'Forbidden',
          })
      )
    )

    const result = await runScriptJson('delete_current_conversation.js')

    expect(result.deleted).toBe(false)
    expect(result.skipped).toBe(false)
    expect(result.status).toBe(403)
    expect(result.error).toBe('permission denied')
  })
})
