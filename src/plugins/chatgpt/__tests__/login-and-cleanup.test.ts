import { beforeEach, describe, expect, it, vi } from 'vitest'

import {
  getCgptState,
  markVisible,
  runScriptJson,
  setCgptState,
} from '../../../../test-utils/chatgpt/script-test-utils'

describe('chatgpt login and cleanup scripts', () => {
  beforeEach(() => {
    history.replaceState(null, '', '/')
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
      api_headers: {
        'oai-device-id': 'device-1',
        'oai-session-id': 'session-1',
      },
    })
    const fetchMock = vi.fn(async (url: string, _init?: RequestInit) => {
      if (String(url).includes('/backend-api/conversations')) {
        return new Response(JSON.stringify({ items: [{ id: 'conv-other' }] }))
      }
      return new Response(JSON.stringify({ success: true }))
    })
    vi.stubGlobal('fetch', fetchMock)

    const result = await runScriptJson('delete_current_conversation.js')
    const [, patchInit] = fetchMock.mock.calls[0]
    const state = getCgptState()

    expect(result.deleted).toBe(true)
    expect(result.verified).toBe(true)
    expect(fetchMock).toHaveBeenCalledWith(
      '/backend-api/conversation/conv-result',
      expect.objectContaining({
        method: 'PATCH',
        credentials: 'include',
        cache: 'no-store',
        referrer: 'http://localhost:3000/c/conv-result',
        headers: expect.objectContaining({
          'content-type': 'application/json',
          'oai-device-id': 'device-1',
          'oai-session-id': 'session-1',
          'x-openai-target-route': '/backend-api/conversation/{conversation_id}',
        }),
      })
    )
    expect(JSON.parse(String((patchInit as RequestInit).body))).toEqual({ is_visible: false })
    expect(fetchMock).toHaveBeenCalledWith(
      expect.stringContaining('/backend-api/conversations?offset=0'),
      expect.objectContaining({ credentials: 'include' })
    )
    expect(state.deleted_conversation_id).toBe('conv-result')
    expect(state.result_conversation_id).toBe('')
    expect(state.conversation_id).toBe('conv-current')
  })

  it('delete_current_conversation falls back to current /c route id', async () => {
    history.replaceState(null, '', '/c/conv-route')
    setCgptState({})
    const fetchMock = vi.fn(async (url: string) => {
      if (String(url).includes('/backend-api/conversations')) {
        return new Response(JSON.stringify({ items: [] }))
      }
      return new Response(JSON.stringify({ success: true }))
    })
    vi.stubGlobal('fetch', fetchMock)

    const result = await runScriptJson('delete_current_conversation.js')

    expect(result.deleted).toBe(true)
    expect(fetchMock).toHaveBeenCalledWith(
      '/backend-api/conversation/conv-route',
      expect.objectContaining({ method: 'PATCH' })
    )
  })

  it('delete_current_conversation prefers current route id over stale runtime id', async () => {
    history.replaceState(null, '', '/c/conv-route')
    setCgptState({ result_conversation_id: 'conv-stale', conversation_id: 'conv-stale' })
    const fetchMock = vi.fn(async (url: string) => {
      if (String(url).includes('/backend-api/conversations')) {
        return new Response(JSON.stringify({ items: [] }))
      }
      return new Response(JSON.stringify({ success: true }))
    })
    vi.stubGlobal('fetch', fetchMock)

    const result = await runScriptJson('delete_current_conversation.js')

    expect(result.deleted).toBe(true)
    expect(fetchMock).toHaveBeenCalledWith(
      '/backend-api/conversation/conv-route',
      expect.objectContaining({ method: 'PATCH' })
    )
    expect(fetchMock).not.toHaveBeenCalledWith(
      '/backend-api/conversation/conv-stale',
      expect.anything()
    )
  })

  it('delete_current_conversation uses sidebar delete menu before api fallback', async () => {
    history.replaceState(null, '', '/c/conv-route')
    document.body.innerHTML = `
      <a data-sidebar-item data-active href="/c/conv-route">
        <button data-conversation-options-trigger="conv-route">options</button>
      </a>
    `
    let uiDeleted = false
    const trigger = document.querySelector('[data-conversation-options-trigger]')
    trigger?.addEventListener('click', () => {
      const item = document.createElement('button')
      item.setAttribute('role', 'menuitem')
      item.textContent = '删除'
      item.addEventListener('click', () => {
        uiDeleted = true
      })
      document.body.appendChild(item)
      markVisible(item)
    })
    setCgptState({ result_conversation_id: 'conv-stale' })
    const fetchMock = vi.fn(async (url: string) => {
      if (String(url).includes('/backend-api/conversations')) {
        const items = uiDeleted ? [] : [{ id: 'conv-route' }]
        return new Response(JSON.stringify({ items }))
      }
      return new Response(JSON.stringify({ success: true }))
    })
    vi.stubGlobal('fetch', fetchMock)

    const result = await runScriptJson('delete_current_conversation.js')

    expect(result.deleted).toBe(true)
    expect(result.ui_delete).toEqual(expect.objectContaining({ attempted: true, ok: true }))
    expect(fetchMock).not.toHaveBeenCalledWith(
      expect.stringContaining('/backend-api/conversation/conv-route'),
      expect.objectContaining({ method: 'PATCH' })
    )
  })

  it('delete_current_conversation reports visible conversation after refresh', async () => {
    setCgptState({ conversation_id: 'conv-current' })
    const fetchMock = vi.fn(async (url: string) => {
      if (String(url).includes('/backend-api/conversations')) {
        return new Response(JSON.stringify({ items: [{ id: 'conv-current' }] }))
      }
      return new Response(JSON.stringify({ success: true }))
    })
    vi.stubGlobal('fetch', fetchMock)

    const result = await runScriptJson('delete_current_conversation.js')

    expect(result.deleted).toBe(false)
    expect(result.verified).toBe(true)
    expect(result.cleanup_error).toBe('conversation_still_visible')
    expect(result.error).toBeUndefined()
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
    expect(result.cleanup_error).toBe('permission denied')
    expect(result.error).toBeUndefined()
  })

  it('delete_current_conversation formats object errors without failing generation flow', async () => {
    setCgptState({ conversation_id: 'conv-current' })
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify({ error: { message: 'token expired' } })))
    )

    const result = await runScriptJson('delete_current_conversation.js')

    expect(result.deleted).toBe(false)
    expect(result.cleanup_error).toBe('token expired')
    expect(result.error).toBeUndefined()
  })
})
