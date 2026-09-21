import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { readFileSync } from 'node:fs'
import ApiDocsPage from './ApiDocsPage'
import { documents } from './catalog'
import { fullDocumentation } from './format'

describe('对外接口文档', () => {
  it('一键复制全文，搜索、分类及剪贴板失败反馈可用', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } })
    render(<ApiDocsPage />)
    fireEvent.click(screen.getByRole('button', { name: '复制全部接口文档' }))
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(fullDocumentation()))
    expect(await screen.findByRole('status')).toHaveTextContent('已复制')
    fireEvent.change(screen.getByRole('textbox', { name: '搜索接口' }), {
      target: { value: '/v1/audio/speech' },
    })
    expect(screen.getByText('1 个接口与调用场景')).toBeInTheDocument()
    writeText.mockRejectedValueOnce(new Error('denied'))
    fireEvent.click(screen.getByRole('button', { name: '复制请求示例 1' }))
    await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('复制失败'))
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'missing-route' } })
    expect(screen.getByText('没有匹配的接口，请换一个关键词。')).toBeInTheDocument()
    fireEvent.change(screen.getByRole('textbox'), { target: { value: '' } })
    fireEvent.click(screen.getByRole('button', { name: documents[0].group }))
    expect(
      screen.getByText(
        `${documents.filter((d) => d.group === documents[0].group).length} 个接口与调用场景`
      )
    ).toBeInTheDocument()
  })

  it('覆盖全部注册的对外路由，不包含管理接口', () => {
    const source = ['main.rs', 'realtime_api.rs', 'generation_queue/api.rs']
      .map((file) => readFileSync(`../server/src/${file}`, 'utf8'))
      .join('\n')
    const normalize = (path: string) => path.split('?')[0].replace(/\{[^}]+\}/g, '{}')
    const listed = new Set(documents.map((doc) => normalize(doc.path)))
    const paths = [...source.matchAll(/"(\/(?:v1\/|api\/v2\/)[^"\s]*)"/g)]
      .map((match) => match[1])
      .filter((path) => path !== '/v1/')
    expect(paths.length).toBeGreaterThan(20)
    for (const path of paths) expect(listed.has(normalize(path)), path).toBe(true)
    for (const doc of documents) {
      expect(doc.path).not.toMatch(/^\/api\/(accounts|proxies|settings)/)
      expect(doc.response).toBeTruthy()
      expect(doc.parameters.length).toBeGreaterThan(0)
    }
  })
})
