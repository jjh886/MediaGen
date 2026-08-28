import { fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { DEFAULT_SETTINGS, type AppSettings } from '@/shared/services/settings-service'
import { PlatformGenerationConfigCard } from '../PlatformGenerationConfigCard'
import {
  readPlatformGenerationConfig,
  writePlatformGenerationConfig,
} from '../platform-generation-config'

const settings: AppSettings = {
  ...DEFAULT_SETTINGS,
  api_ports: '18080',
  platform_generation_configs: '{"other":{"quality":"high"}}',
}

const schema = {
  title: '示例平台生成配置',
  fields: [
    { key: 'model', type: 'text' as const, label: '模型标识', default: '' },
    {
      key: 'thinking_effort',
      type: 'select' as const,
      label: '思考深度',
      default: 'extended',
      options: [
        { value: '', label: '跟随默认' },
        { value: 'standard', label: '标准' },
        { value: 'extended', label: '深度' },
      ],
    },
  ],
}

describe('PlatformGenerationConfigCard', () => {
  const fetchMock = vi.fn()

  beforeEach(() => {
    fetchMock.mockReset()
    fetchMock.mockResolvedValue(
      new Response(
        JSON.stringify({
          data: [
            {
              platform_id: 'platform-a',
              display_name: 'Platform A',
              models: [],
              generation_settings: schema,
            },
          ],
        }),
        { status: 200, headers: { 'Content-Type': 'application/json' } }
      )
    )
    vi.stubGlobal('fetch', fetchMock)
  })

  afterEach(() => vi.unstubAllGlobals())

  it('按插件声明渲染字段，并保留其他平台配置', async () => {
    const update = vi.fn()
    render(<PlatformGenerationConfigCard settings={settings} update={update} />)

    expect(await screen.findByLabelText('Platform A 思考深度')).toHaveValue('extended')
    fireEvent.change(screen.getByLabelText('Platform A 模型标识'), {
      target: { value: 'example-model' },
    })

    const saved = JSON.parse(update.mock.calls[0][1])
    expect(saved.other).toEqual({ quality: 'high' })
    expect(saved['platform-a'].model).toBe('example-model')
  })
})

describe('platform-generation-config', () => {
  it('损坏配置回到插件默认值，写入时不覆盖其他平台', () => {
    expect(readPlatformGenerationConfig('not-json', 'platform-a', schema)).toEqual({
      model: '',
      thinking_effort: 'extended',
    })
    const raw = writePlatformGenerationConfig('{"other":{"enabled":true}}', 'platform-a', {
      model: '',
      thinking_effort: 'standard',
    })

    expect(JSON.parse(raw)).toEqual({
      other: { enabled: true },
      'platform-a': { model: '', thinking_effort: 'standard' },
    })
  })
})
