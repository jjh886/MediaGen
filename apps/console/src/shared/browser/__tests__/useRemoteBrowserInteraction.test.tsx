import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { useState } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useRemoteBrowserInteraction } from '../useRemoteBrowserInteraction'

const mocks = vi.hoisted(() => ({
  history: vi.fn(),
  copy: vi.fn(),
  cut: vi.fn(),
  selectAll: vi.fn(),
  write: vi.fn(),
}))

vi.mock('@/shared/services/browser-session-service', () => ({
  browserSessionHistory: (...args: unknown[]) => mocks.history(...args),
  copyBrowserSelection: (...args: unknown[]) => mocks.copy(...args),
  cutBrowserSelection: (...args: unknown[]) => mocks.cut(...args),
  selectAllBrowserContent: (...args: unknown[]) => mocks.selectAll(...args),
  writeBrowserClipboard: (...args: unknown[]) => mocks.write(...args),
}))

function Harness({ sendInput = vi.fn() }: { sendInput?: (value: unknown) => void }) {
  const [, setStatus] = useState('')
  const interaction = useRemoteBrowserInteraction({
    sessionId: 'session-1',
    viewport: { width: 1000, height: 500 },
    sendInput,
    onStatus: setStatus,
  })
  return (
    <div>
      <canvas data-testid="canvas" {...interaction.pointerHandlers} />
      <textarea aria-label="input" ref={interaction.sinkRef} {...interaction.keyHandlers} />
      {interaction.menu ? (
        <button onClick={() => interaction.runContextAction('reload')}>菜单</button>
      ) : null}
    </div>
  )
}

describe('useRemoteBrowserInteraction', () => {
  beforeEach(() => {
    Object.values(mocks).forEach((mock) => mock.mockReset())
    mocks.copy.mockResolvedValue('selected')
    mocks.cut.mockResolvedValue('cut text')
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: {
        readText: vi.fn().mockResolvedValue('pasted'),
        writeText: vi.fn().mockResolvedValue(undefined),
      },
    })
  })

  it('blocks the canvas save menu and provides remote browser actions', () => {
    render(<Harness />)
    const canvas = screen.getByTestId('canvas')
    expect(fireEvent.contextMenu(canvas, { clientX: 20, clientY: 30 })).toBe(false)
    fireEvent.click(screen.getByText('菜单'))
    expect(mocks.history).toHaveBeenCalledWith('session-1', 'reload')
  })

  it('routes clipboard shortcuts without affecting the management page', async () => {
    render(<Harness />)
    const input = screen.getByLabelText('input')
    fireEvent.keyDown(input, { key: 'c', ctrlKey: true })
    fireEvent.keyDown(input, { key: 'v', metaKey: true })
    fireEvent.keyDown(input, { key: 'a', ctrlKey: true })

    await waitFor(() => expect(mocks.copy).toHaveBeenCalledWith('session-1'))
    expect(mocks.write).toHaveBeenCalledWith('session-1', 'pasted')
    expect(mocks.selectAll).toHaveBeenCalledWith('session-1')
  })

  it('sends composition updates and committed Chinese text', () => {
    const sendInput = vi.fn()
    render(<Harness sendInput={sendInput} />)
    const input = screen.getByLabelText('input')
    fireEvent.compositionStart(input)
    fireEvent.compositionUpdate(input, { data: '中文' })
    fireEvent.compositionEnd(input, { data: '中文' })

    expect(sendInput).toHaveBeenCalledWith(
      expect.objectContaining({ kind: 'ime', event: 'set_composition', text: '中文' })
    )
    expect(sendInput).toHaveBeenCalledWith(
      expect.objectContaining({ kind: 'ime', event: 'commit', text: '中文' })
    )
  })
})
