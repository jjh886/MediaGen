import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { fetchPlugins } from '../plugin-service'

function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  })
}

describe('fetchPlugins', () => {
  const fetchMock = vi.fn()

  beforeEach(() => {
    fetchMock.mockReset()
    vi.stubGlobal('fetch', fetchMock)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('从服务端插件 API 读取并转换模型数量', async () => {
    fetchMock.mockResolvedValueOnce(
      jsonResponse({
        data: [
          {
            platform_id: 'chatgpt',
            display_name: 'ChatGPT',
            models: [{ model_id: 'gpt-image' }, { model_id: 'gpt-video' }],
          },
        ],
      })
    )

    const plugins = await fetchPlugins()

    expect(fetchMock).toHaveBeenCalledWith('/api/plugins', {
      headers: { Accept: 'application/json' },
    })
    expect(plugins).toEqual([
      {
        platform_id: 'chatgpt',
        display_name: 'ChatGPT',
        model_count: 2,
        supports_registration: false,
      },
    ])
  })

  it('优先使用服务端明确返回的 model_count 和 supports_registration', async () => {
    fetchMock.mockResolvedValueOnce(
      jsonResponse({
        data: [
          {
            platform_id: 'custom',
            display_name: 'Custom',
            model_count: 7,
            supports_registration: true,
            models: [],
          },
        ],
      })
    )

    const plugins = await fetchPlugins()

    expect(plugins[0]).toMatchObject({
      model_count: 7,
      supports_registration: true,
    })
  })
})
