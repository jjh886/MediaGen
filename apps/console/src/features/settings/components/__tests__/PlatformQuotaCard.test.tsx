import { render, screen, within } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
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
  flow_mirror_url: 'https://flow.example.com',
  check_interval_minutes: 7,
  cooldown_default_seconds: 42,
  cache_dir: '/tmp/mediagen-test-cache',
  platform_tier_overrides: JSON.stringify({}),
}

describe('PlatformQuotaCard', () => {
  it('显示所有支持平台的免费账号默认配额', () => {
    render(<PlatformQuotaCard settings={settings} update={vi.fn()} />)

    const expectedRows = [
      { label: 'Flow 免费', values: ['50', '0', '1', '0'] },
      { label: '豆包 免费', values: ['60', '0', '1', '0'] },
      { label: 'ChatGPT 免费', values: ['40', '0', '1', '5'] },
      { label: 'codex 免费', values: ['40', '0', '1', '0'] },
      { label: '即梦 免费', values: ['100', '0', '3', '0'] },
    ]

    for (const row of expectedRows) {
      const rowElement = screen.getByText(row.label).closest('div')
      expect(rowElement).not.toBeNull()
      const inputs = within(rowElement!).getAllByRole('spinbutton')
      expect(inputs.map((input) => (input as HTMLInputElement).value)).toEqual(row.values)
    }
  })
})
