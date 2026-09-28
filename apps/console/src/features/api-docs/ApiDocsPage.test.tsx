import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { readFileSync } from 'node:fs'
import ApiDocsPage from './ApiDocsPage'
import { documents } from './catalog'
import { fullDocumentation } from './format'
import { guideSections } from './guide'

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

  it('全部视图按指定顺序分组，搜索隐藏空分组，单分类只显示该组接口', () => {
    render(<ApiDocsPage />)
    const order = ['模型', '文本', '图片', '视频', '音频', '实时语音', '文件', '排队']
    const filters = screen.getByRole('group', { name: '接口分类' })
    expect(
      within(filters)
        .getAllByRole('button')
        .map((button) => button.textContent)
    ).toEqual(['全部', ...order])
    const headings = () =>
      screen
        .queryAllByRole('heading', { level: 2 })
        .map((heading) => heading.textContent)
        .filter((name) => order.includes(name || ''))
    expect(headings()).toEqual(order)
    for (const name of order) {
      const section = screen.getByRole('region', { name: `${name}接口` })
      for (const doc of documents.filter((doc) => doc.group === name))
        expect(within(section).getAllByText(doc.title).length).toBeGreaterThan(0)
    }
    fireEvent.change(screen.getByRole('textbox', { name: '搜索接口' }), {
      target: { value: '豆包' },
    })
    expect(headings()).toEqual(['视频'])
    expect(screen.getByText('豆包文生视频')).toBeInTheDocument()
    fireEvent.click(within(filters).getByRole('button', { name: '视频' }))
    expect(headings()).toEqual([])
    expect(screen.getByRole('region', { name: '视频接口' })).toBeInTheDocument()
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'missing-route' } })
    expect(screen.queryByRole('region', { name: '视频接口' })).not.toBeInTheDocument()
  })

  it('豆包视频示例模型与插件保持一致，参考视频字段进入复制全文', () => {
    const manifest = JSON.parse(readFileSync('../../plugins/doubao-api/manifest.json', 'utf8')) as {
      models: { model_id: string; task_kind: string }[]
    }
    const markdown = fullDocumentation()
    for (const model of manifest.models.filter((model) => model.task_kind === 'video'))
      expect(markdown).toContain(model.model_id)
    expect(markdown).toContain('video_reference.file_id')
    expect(markdown).toContain('video_reference.file_data')
    expect(markdown).toContain('input_reference=@reference.png')
    const scenes = documents.filter((doc) => doc.title.startsWith('豆包'))
    expect(scenes).toHaveLength(3)
    for (const doc of scenes) {
      const model =
        doc.body?.model || doc.form?.find((field) => field.startsWith('model='))?.slice(6)
      expect(
        manifest.models.some((entry) => entry.model_id === model && entry.task_kind === 'video')
      ).toBe(true)
      expect(doc.path).toBe('/v1/videos')
    }
  })

  it('会话名称字段、结束帧与平台范围进入文档及复制全文', () => {
    const markdown = fullDocumentation()
    expect(markdown).toContain('metadata.conversation_title')
    expect(markdown).toContain('metadata.conversation_id')
    expect(markdown).toContain('Chat 流式结束帧示例')
    expect(markdown).toContain('Codex 当前不提供')
    for (const path of ['/v1/responses', '/v1/chat/completions']) {
      const doc = documents.find((doc) => doc.path === path && doc.method === 'POST')!
      expect(doc.responseFields.some((field) => field.name === 'metadata.conversation_title')).toBe(
        true
      )
      expect(
        doc.streamFields?.some((field) => field.name.endsWith('metadata.conversation_title'))
      ).toBe(true)
    }
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
      expect(doc.requestFields.length).toBeGreaterThan(0)
    }
  })
  it('逐接口提供结构化字段表，复制全文保留 Markdown 表格', () => {
    const markdown = fullDocumentation()
    expect(markdown).toContain('| 参数名 | 类型 | 是否必填 | 默认值 | 说明 |')
    expect(markdown).toContain('| 字段名 | 类型 | 是否必返 / 条件 | 固定值 / 空值 | 说明 |')
    expect(markdown).toContain('string \\| array<object>')
    for (const doc of documents) {
      expect(doc.requestFields.some((f) => f.name === 'Authorization' && f.required === '是')).toBe(
        true
      )
      expect(doc.responseFields.length, doc.path).toBeGreaterThan(0)
      for (const list of [doc.requestFields, doc.responseFields, doc.streamFields || []]) {
        expect(new Set(list.map((f) => `${f.location}:${f.name}`)).size).toBe(list.length)
        for (const field of list) {
          expect(field.type.trim()).not.toBe('')
          expect(field.required.trim()).not.toBe('')
          expect(field.defaultValue.trim()).not.toBe('')
          expect(field.description.trim()).not.toBe('')
        }
      }
      if (doc.body)
        for (const name of Object.keys(doc.body)) {
          expect(
            doc.requestFields.some((f) => f.name === name || f.name.startsWith(name + '.')),
            `${doc.path}: ${name}`
          ).toBe(true)
        }
      if (typeof doc.response === 'object' && doc.response)
        for (const name of Object.keys(doc.response)) {
          expect(
            doc.responseFields.some((f) => f.name === name || f.name.startsWith(name + '.')),
            `${doc.path}: ${name}`
          ).toBe(true)
        }
    }
  })

  it('展示请求表、默认值、响应表和流式字段，支持按字段名搜索', () => {
    render(<ApiDocsPage />)
    fireEvent.change(screen.getByRole('textbox', { name: '搜索接口' }), {
      target: { value: '/v1/chat/completions' },
    })
    const request = screen.getByRole('table', { name: '请求体参数' })
    expect(within(request).getByRole('columnheader', { name: '是否必填' })).toBeInTheDocument()
    expect(within(request).getByRole('row', { name: /store boolean 否 false/ })).toBeInTheDocument()
    expect(screen.getByRole('table', { name: '响应字段 · HTTP 200' })).toBeInTheDocument()
    expect(screen.getByRole('table', { name: '流式响应字段 · stream=true' })).toBeInTheDocument()
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'partial_images' } })
    expect(screen.queryByText('没有匹配的接口，请换一个关键词。')).not.toBeInTheDocument()
    expect(screen.getAllByText('partial_images').length).toBeGreaterThan(0)
  })

  it('区分图片默认模型、Responses 保存策略、文件分页和 Live 响应格式', () => {
    const find = (path: string, method = 'POST') =>
      documents.find((d) => d.path === path && d.method === method)!
    const value = (path: string, field: string, method = 'POST') =>
      find(path, method).requestFields.find((f) => f.name === field)?.defaultValue
    expect(value('/v1/images/generations', 'model')).toBe('gpt-image-1')
    expect(value('/v1/responses', 'store')).toBe('true')
    expect(value('/v1/responses', 'store', 'WS')).toBe('false')
    expect(value('/v1/files', 'limit', 'GET')).toBe('10000')
    expect(find('/v1/live/sessions').status).toBe(201)
    expect(find('/v1/live').requestFields.some((f) => f.name === 'sdp')).toBe(true)
    expect(find('/v1/live').response).toContain('application/sdp')
  })
  it('调用指南以章节、步骤和代码块展示，全文复制包含全部章节', () => {
    render(<ApiDocsPage />)
    fireEvent.click(screen.getByText('调用约定、排队流程、流式输出和实时语音'))
    const navigation = screen.getByRole('navigation', { name: '调用指南章节' })
    expect(screen.getByRole('table', { name: '请求与响应约定' })).toBeVisible()
    fireEvent.click(within(navigation).getByRole('button', { name: '图片排队' }))
    expect(screen.getByRole('heading', { name: '保存任务 ID' })).toBeVisible()
    expect(screen.getByText('入队成功 · HTTP 202')).toBeVisible()
    fireEvent.click(within(navigation).getByRole('button', { name: '流式输出' }))
    expect(screen.getByRole('table', { name: '不同接口的流式读取方式' })).toBeVisible()
    fireEvent.click(within(navigation).getByRole('button', { name: '实时语音' }))
    expect(screen.getByRole('heading', { name: '持续发送音频' })).toBeVisible()
    expect(screen.getByText('手动结束输入 · 先发送')).toBeVisible()
    const markdown = fullDocumentation()
    for (const section of guideSections) {
      expect(markdown).toContain('## ' + section.title)
      for (const block of section.blocks)
        if (block.kind === 'code') {
          expect(markdown).toContain('```' + block.language + '\n' + block.value)
          if (block.language === 'json') expect(() => JSON.parse(block.value)).not.toThrow()
        }
    }
  })
})
