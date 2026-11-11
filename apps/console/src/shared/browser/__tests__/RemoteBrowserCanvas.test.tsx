import { render, screen } from '@testing-library/react'
import { createRef } from 'react'
import { describe, expect, it, vi } from 'vitest'
import RemoteBrowserCanvas from '../RemoteBrowserCanvas'

describe('RemoteBrowserCanvas', () => {
  it.each(['远程浏览器已连接', '远程输入请求超时，请检查输入框内容后重试'])(
    '网页画面显示后不叠加状态区域：%s',
    (statusText) => {
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
          statusText={statusText}
        />
      )

      expect(screen.queryByRole('status')).not.toBeInTheDocument()
      expect(screen.queryByText(statusText)).not.toBeInTheDocument()
      expect(screen.getByLabelText('远程网页输入')).toBeInTheDocument()
    }
  )
})
