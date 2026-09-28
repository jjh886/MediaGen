import { render, screen } from '@testing-library/react'
import { createRef } from 'react'
import { describe, expect, it, vi } from 'vitest'
import RemoteBrowserCanvas from '../RemoteBrowserCanvas'

describe('RemoteBrowserCanvas', () => {
  it('keeps input failures visible after the login page has a frame', () => {
    render(
      <RemoteBrowserCanvas
        canvasRef={createRef()}
        sinkRef={createRef()}
        viewport={{ width: 800, height: 600 }}
        deviceScaleFactor={1}
        pointer={{
          cursor: 'text',
          editable: true,
          selectable: true,
          has_selection: false,
          password: false,
          element_kind: 'input',
        }}
        pointerHandlers={{}}
        keyHandlers={{}}
        menu={null}
        onMenuClose={vi.fn()}
        onMenuAction={vi.fn()}
        hasFrame
        statusText="远程输入请求超时，请检查输入框内容后重试"
      />
    )

    expect(screen.getByRole('status')).toHaveTextContent('远程输入请求超时，请检查输入框内容后重试')
    expect(screen.getByLabelText('远程网页输入')).toBeInTheDocument()
  })
})
