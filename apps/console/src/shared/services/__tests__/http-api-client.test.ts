import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { HttpApiError, apiGet } from '../http-api-client'

describe('http-api-client', () => {
  const fetchMock = vi.fn()

  beforeEach(() => {
    fetchMock.mockReset()
    vi.stubGlobal('fetch', fetchMock)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('正常解包服务端 data 字段', async () => {
    fetchMock.mockResolvedValueOnce(
      new Response(JSON.stringify({ data: { ok: true } }), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })
    )

    await expect(apiGet('/health')).resolves.toEqual({ ok: true })
  })

  it('后端返回纯文本 404 时不会再抛 JSON 解析错误', async () => {
    fetchMock.mockResolvedValueOnce(
      new Response('Request did not match any route', { status: 404 })
    )

    await expect(apiGet('/api/missing')).rejects.toMatchObject({
      status: 404,
      message: 'Request did not match any route',
    } satisfies Partial<HttpApiError>)
  })
})
