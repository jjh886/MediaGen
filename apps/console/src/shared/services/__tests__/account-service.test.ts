import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  fetchAccount,
  fetchAccounts,
  fetchAccountSummary,
  fetchAllAccounts,
} from '../account-service'

function jsonResponse(data: unknown): Response {
  return new Response(JSON.stringify({ data }), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  })
}

describe('shared account-service', () => {
  const fetchMock = vi.fn()

  beforeEach(() => {
    fetchMock.mockReset()
    vi.stubGlobal('fetch', fetchMock)
  })

  afterEach(() => vi.unstubAllGlobals())

  it('无平台筛选时只携带默认分页参数', async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse([]))

    await fetchAccounts()

    expect(fetchMock).toHaveBeenCalledWith('/api/accounts?limit=40&offset=0', {
      headers: { Accept: 'application/json' },
    })
  })

  it('非列表用途会连续读取全部账号', async () => {
    const firstPage = Array.from({ length: 100 }, (_, index) => ({ id: `acc-${index}` }))
    fetchMock
      .mockResolvedValueOnce(jsonResponse(firstPage))
      .mockResolvedValueOnce(jsonResponse([{ id: 'acc-100' }]))

    const accounts = await fetchAllAccounts('codex')

    expect(accounts).toHaveLength(101)
    expect(fetchMock.mock.calls.map(([url]) => url)).toEqual([
      '/api/accounts?limit=100&offset=0&platform=codex',
      '/api/accounts?limit=100&offset=100&platform=codex',
    ])
  })

  it('按 ID 和摘要接口读取不受列表分页影响', async () => {
    fetchMock
      .mockResolvedValueOnce(jsonResponse({ id: 'acc/a' }))
      .mockResolvedValueOnce(jsonResponse({ available: 2, total: 3 }))

    await expect(fetchAccount('acc/a')).resolves.toMatchObject({ id: 'acc/a' })
    await expect(fetchAccountSummary()).resolves.toEqual({ available: 2, total: 3 })
    expect(fetchMock.mock.calls.map(([url]) => url)).toEqual([
      '/api/accounts/acc%2Fa',
      '/api/accounts/summary',
    ])
  })
})
