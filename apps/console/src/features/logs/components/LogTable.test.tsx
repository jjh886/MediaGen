import { render, screen } from '@testing-library/react'
import { expect, it, vi } from 'vitest'
import { LogTable } from './LogTable'

it('日志时间转换为 UTC+8，跨日显示且不改变日志原始数据', () => {
  const log = {
    id: 'log-1',
    timestamp: '2026-11-10T20:15:30+00:00',
    module: 'engine',
    level: 'info' as const,
    summary: '任务完成',
  }
  render(
    <LogTable
      logs={[log]}
      loading={false}
      expandedId={null}
      onToggle={vi.fn()}
      hasMore={false}
      loadingMore={false}
      onLoadMore={vi.fn()}
    />
  )
  expect(screen.getByText('2026/11/11 04:15:30')).toBeInTheDocument()
  expect(screen.queryByText(log.timestamp)).not.toBeInTheDocument()
  expect(log.timestamp).toBe('2026-11-10T20:15:30+00:00')
})
