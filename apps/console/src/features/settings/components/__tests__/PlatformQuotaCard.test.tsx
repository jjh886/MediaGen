import { render, screen, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { PlatformQuotaCard } from '../PlatformQuotaCard'
import type { AppSettings } from '@/shared/services/settings-service'

const settings: AppSettings = {
  api_port: 17777,
  api_end_port: 17779,
  api_ports: '17777-17779',
  api_port_count: 3,
  api_address: '0.0.0.0',
  api_allowed_origins: 'https://client.example.com',
  image_task_concurrency: 8,
  image_task_queue_limit: 32,
  check_interval_minutes: 7,
  cooldown_default_seconds: 42,
  cache_dir: '/tmp/mediagen-test-cache',
  platform_tier_overrides: JSON.stringify({}),
}

describe('PlatformQuotaCard', () => {
  const fetchMock = vi.fn()

  beforeEach(() => {
    fetchMock.mockReset()
    vi.stubGlobal('fetch', fetchMock)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('显示所有支持平台的免费账号默认配额', async () => {
    fetchMock.mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          data: [
            {
              platform_id: 'flow',
              display_name: 'Flow',
              tiers: {
                free: {
                  daily_limit: 50,
                  total_limit: 0,
                  concurrency_limit: 1,
                  task_interval_seconds: 0,
                },
              },
            },
            {
              platform_id: 'chatgpt',
              display_name: 'ChatGPT',
              tiers: {
                free: {
                  daily_limit: 40,
                  total_limit: 0,
                  concurrency_limit: 1,
                  task_interval_seconds: 5,
                },
              },
            },
          ],
        }),
        { status: 200, headers: { 'Content-Type': 'application/json' } }
      )
    )
    render(<PlatformQuotaCard settings={settings} update={vi.fn()} />)

    const expectedRows = [
      { label: 'Flow 免费', values: ['50', '0', '1', '0'] },
      { label: 'ChatGPT 免费', values: ['40', '0', '1', '5'] },
    ]

    for (const row of expectedRows) {
      const rowElement = (await screen.findByText(row.label)).closest('div')
      expect(rowElement).not.toBeNull()
      const inputs = within(rowElement!).getAllByRole('spinbutton')
      expect(inputs.map((input) => (input as HTMLInputElement).value)).toEqual(row.values)
    }
  })
})
