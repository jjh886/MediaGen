import { readFileSync } from 'node:fs'
import vm from 'node:vm'
import crypto from 'node:crypto'
import { describe, expect, it, vi } from 'vitest'

const manifest = JSON.parse(readFileSync('../../plugins/gemini/manifest.json', 'utf8'))
function runtime(flow = 'generate_text') {
  let checkpoint = { phase: 'ready' }
  const host = {
    emit: vi.fn(),
    checkpoint(value) {
      if (value) checkpoint = value
      return checkpoint
    },
    call: vi.fn((action, params) => {
      if (action === 'time.sleep') return {}
      if (action === 'crypto.base64_encode_utf8')
        return { value: Buffer.from(params.text).toString('base64') }
      if (action === 'crypto.base64_encode_latin1')
        return { value: Buffer.from(params.text, 'latin1').toString('base64') }
      if (action === 'crypto.base64url_decode_utf8')
        return { value: Buffer.from(params.text, 'base64url').toString('utf8') }
      if (action === 'crypto.base64_decode_latin1')
        return { value: Buffer.from(params.text, 'base64').toString('latin1') }
      if (action === 'crypto.digest_bytes')
        return {
          value: crypto
            .createHash(params.algorithm)
            .update(Buffer.from(params.data_base64, 'base64'))
            .digest('base64'),
        }
      throw new Error('unexpected host action: ' + action)
    }),
  }
  const context = vm.createContext({ host })
  for (const file of manifest.flows[flow].steps[0].script_files)
    vm.runInContext(
      readFileSync(
        file.startsWith('@shared/')
          ? '../../plugins/_shared/' + file.slice(8)
          : '../../plugins/gemini/scripts/' + file,
        'utf8'
      ),
      context
    )
  return { context, host }
}

describe('Gemini API 插件', () => {
  it('完整声明三种生成流程，指纹由宿主提供而非插件拼装', () => {
    expect(JSON.parse(readFileSync('../../plugins/plugins.index.json', 'utf8')).plugins).toContain(
      'gemini'
    )
    for (const flow of Object.keys(manifest.flows)) expect(() => runtime(flow)).not.toThrow()
    const { context } = runtime()
    const headers = context.geminiHeaders(
      {
        auth: {},
        cookies: [{ name: 'SAPISID', value: 'test-session', domain: '.google.com', path: '/' }],
      },
      'https://gemini.google.com/app'
    )
    expect(headers.authorization).toMatch(/^SAPISIDHASH \d+_[a-f0-9]{40}$/)
    expect(headers['user-agent']).toBeUndefined()
    expect(headers['sec-ch-ua']).toBeUndefined()
    expect(manifest.protected_json_files).toContain('auth.json')
  })

  it('多附件在同一个字段重复编码，不把第二个附件误放成另一个字段', () => {
    const { context } = runtime()
    const payload = context.geminiPayload(
      '',
      { mode: 1, think: 1, tool: 0 },
      [
        [['/file-one', 1], 'one.png'],
        [['/file-two', 1], 'two.png'],
      ],
      null
    )
    const message = context.geminiWireMessage(
      context.geminiWireDecode(context.geminiWireInner(payload)),
      1
    )
    expect(message[4]).toHaveLength(2)
    expect(context.geminiWireText(context.geminiWireDecode(message[4][0]), 2)).toBe('one.png')
    expect(context.geminiWireText(context.geminiWireDecode(message[4][1]), 2)).toBe('two.png')
  })

  it('只按账号目录主编号和模式发起请求，不依赖参考项目旧常量', () => {
    const { context } = runtime()
    const row = Array(20).fill(null)
    row[0] = '1234567890abcdef'
    row[11] = '4 Argon'
    row[17] = 8
    const data = Array(25).fill(null)
    data[15] = [row]
    data[24] = [
      'policy',
      [
        [
          [null, 8],
          [
            [1, '低'],
            [4, '中'],
            [2, '高'],
          ],
        ],
      ],
    ]
    context.geminiRpc = () => data
    expect(context.geminiModel({}, 'gemini-4-argon', 'medium')).toMatchObject({
      hex: row[0],
      mode: 8,
      think: 4,
    })
    expect(() => context.geminiModel({}, 'gemini-not-available')).toThrow(
      'account_model_unavailable'
    )
    const session = {}
    data[16] = [8]
    context.geminiCatalog(session)
    expect(session.account_header).toEqual([12, 2])
    data[16] = [16]
    context.geminiCatalog(session)
    expect(session.account_header).toEqual([12, 3])
    data[16] = [21]
    context.geminiCatalog(session)
    expect(session.account_header).toEqual([13, 1])
    data[14] = 1016
    expect(() => context.geminiCatalog(session)).toThrow('LOGIN_EXPIRED')
    data[14] = 1033
    expect(() => context.geminiCatalog(session)).toThrow('verification_required')
  })

  it('中文用途说明不改变模型 ID，使用目录中的真实模型名称', () => {
    const { context } = runtime()
    const row = Array(20).fill(null)
    row[0] = '56fdd199312815e2'
    row[2] = '3.8 Flash'
    row[11] = '全方位帮助'
    row[12] = '3.8 Flash'
    row[17] = 1
    const data = Array(25).fill(null)
    data[15] = [row]
    context.geminiRpc = () => data
    expect(context.geminiModel({}, 'gemini-3.8-flash')).toMatchObject({
      id: 'gemini-3.8-flash',
      name: '3.8 Flash',
      hex: row[0],
    })
  })

  it('按账号选择策略区分 Argon 与 Carbon，不把锁定的基础型号当成可用模型', () => {
    const { context } = runtime()
    const row = Array(20).fill(null)
    row[0] = '95a4c0abcba24364'
    row[12] = '4 Carbon'
    row[17] = 8
    const policy = (variant, name) => [
      [null, 8, false, variant, name, name],
      [[1], [4], [2]],
    ]
    const data = Array(25).fill(null)
    data[15] = [row]
    data[24] = ['carbon_multi_ultra_pro_policy', [policy('argon', '4 Argon')]]
    context.geminiRpc = () => data
    const model = context.geminiModel({}, 'gemini-4-argon', 'medium')
    expect(model).toMatchObject({
      id: 'gemini-4-argon',
      name: '4 Argon',
      hex: row[0],
      mode: 8,
      selector: 'argon',
      variant: 'argon',
      think: 4,
    })
    expect(() => context.geminiModel({}, 'gemini-4-carbon')).toThrow('account_model_unavailable')
    data[24][1].push(policy('carbon', '4 Carbon'))
    expect(context.geminiModel({}, 'gemini-4-carbon')).toMatchObject({
      selector: 'carbon',
      variant: 'carbon',
      name: '4 Carbon',
    })
    const frame = Array(43).fill(null)
    frame[42] = '4 Carbon'
    expect(() =>
      context.geminiParseGeneration(
        JSON.stringify([['wrb.fr', null, JSON.stringify(frame)]]),
        model
      )
    ).toThrow('模型与请求不符')
    data[24][1] = []
    expect(context.geminiCatalog({})).toEqual([])
  })

  it('免费账号自动模式使用稳定 ID，不再把中文用途转换成 gemini--', () => {
    const { context } = runtime()
    const row = Array(20).fill(null)
    row[0] = 'cf41b0e0dd7d53e5'
    row[1] = '自动'
    row[12] = '根据需求调整回答'
    row[17] = 6
    const data = Array(25).fill(null)
    data[15] = [row]
    data[24] = [
      'launch_free_policy',
      [
        [
          [null, 6, false, null, '自动', '根据需求调整回答'],
          [[1], [4], [2]],
        ],
      ],
    ]
    context.geminiRpc = () => data
    const model = context.geminiModel({}, 'gemini-auto')
    expect(model).toMatchObject({ name: '自动', automatic: true, selector: row[0] })
    const frame = Array(43).fill(null)
    frame[42] = '3.5 Flash-Lite'
    expect(() =>
      context.geminiParseGeneration(
        JSON.stringify([['wrb.fr', null, JSON.stringify(frame)]]),
        model
      )
    ).not.toThrow()
  })

  it('解析中文和连续长度帧，不把 UTF-16 长度误当 UTF-8 字节数', () => {
    const { context } = runtime()
    const first = JSON.stringify([[1, ['中文🙂']]])
    const last = JSON.stringify([[2, ['close']]])
    expect(
      context.geminiChannelFrames(first.length + '\n' + first + last.length + '\n' + last + '\n')
    ).toHaveLength(2)
    expect(() => context.geminiChannelFrames('3\n[[')).toThrow('invalid_response')
  })

  it('媒体请求保留空工具参数，视频宽高比使用 packed enum，支持稀疏可选字段', () => {
    const { context } = runtime()
    const image = context.geminiWireDecode(
      context.geminiWireInner(context.geminiPayload('', { mode: 1, think: 1, tool: 14 }, [], null))
    )
    expect(image[55]).toEqual([[]])
    expect(image[56]).toEqual([[]])
    const video = context.geminiWireDecode(
      context.geminiWireInner(
        context.geminiPayload('', { mode: 1, think: 1, tool: 11 }, [], null, '16:9')
      )
    )
    expect(video[56]).toEqual([[10, 1, 16]])
    expect(context.geminiWireMessage(video, 1)[10]).toEqual([[58, 4, 10, 2, 32, 1]])
  })

  it('二进制上下文 token 转成 base64，继续会话时还原 bytes 而不是 UTF-8 文本', () => {
    const { context } = runtime()
    const bytes = [255, 0, 128, 10]
    let encoded = context.geminiWireEncode([[27, bytes]])
    for (const field of [4, 4, 1, 7, 2]) encoded = context.geminiWireEncode([[field, encoded]])
    const frame = context.geminiWireEvent([context.geminiWireBase64(encoded)]).frame
    expect(frame[26]).toBe(Buffer.from(bytes).toString('base64'))
    const payload = context.geminiPayload('', { mode: 1, think: 1, tool: 0 }, [], {
      conversation_id: 'c_test',
      response_id: 'r_test',
      candidate_id: 'rc_test',
      token: frame[26],
    })
    const previous = context.geminiWireMessage(
      context.geminiWireDecode(context.geminiWireInner(payload)),
      3
    )
    expect(previous[10]).toEqual([bytes])
  })

  it('生成请求的第 31 字段使用网页的 packed enum 编码，不嵌套消息', () => {
    const { context } = runtime()
    const payload = context.geminiPayload('', { mode: 1, think: 1, tool: 0 }, [], null)
    const fields = context.geminiWireDecode(context.geminiWireInner(payload))
    // 来自成功网页请求：tag=31/wire=2，长度=3，值为 4、16、19。
    expect(fields[31]).toEqual([[4, 16, 19]])
    expect(fields[3]).toBeUndefined()
    expect(fields[50]).toEqual([0])
    expect(fields[80]).toEqual([1])
    expect(fields[81]).toEqual([1])
    expect(fields[99]).toEqual([1])
    expect(fields[97]).toEqual([0])
    expect(context.geminiWireMessage(fields, 7)[1]).toEqual([0])
    expect(fields[69]).toEqual([1])
    expect(context.geminiWireMessage(fields, 1)[1]).toEqual([[]])
  })

  it.each(['', 'argon', 'carbon'])(
    '按手动顺序检查额度与变体 %s，确认后发送提示词并关闭通道',
    (variant) => {
      const { context, host } = runtime()
      const requests = []
      const rpcs = []
      context.geminiRpc = (_session, rpc, _args, headers) => {
        const metadata = JSON.parse(headers['x-goog-ext-525001261-jspb'])
        expect(metadata[24] || '').toBe(variant)
        if (rpc === 'aPya6c') expect(metadata[4]).toBe(variant || '56fdd199312815e2')
        rpcs.push(rpc)
        return rpc === 'aPya6c' ? [false, 0, []] : []
      }
      context.geminiHeaders = () => ({ authorization: 'test-only' })
      context.geminiRequest = (_session, url, options) => {
        requests.push({ url: new URL(url), ...options })
        const raw = JSON.stringify([[0, ['c', 'test-session']]])
        return {
          ok: true,
          body_text: raw.length + '\n' + raw,
          headers: { 'x-http-session-id': 'test' },
        }
      }
      host.stream = (_action, params, receive) => {
        expect(new URL(params.url).searchParams.get('CI')).toBe('0')
        expect(requests.filter((r) => r.body?.ofs !== undefined).map((r) => r.body.ofs)).toEqual([
          0,
        ])
        const setup = context.geminiWireMessage(
          context.geminiWireDecode([...Buffer.from(requests.at(-1).body.req0___data__, 'base64')]),
          1
        )
        const meta = context.geminiWireMessage(setup, 1)
        expect(context.geminiWireText(meta, 2)).toBe(variant || '56fdd199312815e2')
        const header = requests[0].url.searchParams
          .get('$httpHeaders')
          .split('\r\n')
          .find((line) => line.startsWith('x-goog-ext-525001261-bin:'))
          .split(':')[1]
        const wireHeader = context.geminiWireDecode([...Buffer.from(header, 'base64')])
        expect(context.geminiWireText(wireHeader, 5)).toBe(variant || '56fdd199312815e2')
        expect(context.geminiWireText(wireHeader, 25)).toBe(variant)
        const inner = context.geminiWireMessage(
          context.geminiWireMessage(context.geminiWireMessage(setup, 8), 5),
          1
        )
        expect(context.geminiWireText(meta, 4)).toBe(context.geminiWireText(inner, 60))
        const turn = requests[0].url.searchParams
          .get('$httpHeaders')
          .split('\r\n')
          .find((line) => line.startsWith('x-goog-ext-525005358-bin:'))
          .split(':')[1]
        expect(
          context.geminiWireText(context.geminiWireDecode([...Buffer.from(turn, 'base64')]), 1)
        ).toBe(context.geminiWireText(inner, 60))
        receive({ type: 'http_response', ok: true })
        receive([[1, [context.geminiWireBase64(context.geminiWireEncode([[1, []]]))]]])
        expect(requests.at(-1).body.ofs).toBe(1)
        expect(requests.at(-1).url.searchParams.get('AID')).toBe('1')
        const message = context.geminiWireMessage(
          context.geminiWireDecode([...Buffer.from(requests.at(-1).body.req0___data__, 'base64')]),
          2
        )
        expect(context.geminiWireText(context.geminiWireMessage(message, 9), 1)).toBe('1+1=?')
        const done = context.geminiWireEncode([
          [
            2,
            context.geminiWireEncode([
              [7, context.geminiWireEncode([[1, context.geminiWireEncode([[1, 1]])]])],
            ]),
          ],
        ])
        receive([[2, [context.geminiWireBase64(done)]]])
        receive([[3, [context.geminiWireBase64(context.geminiWireEncode([[4, []]]))]]])
        return { ok: true }
      }
      const model = {
        id: 'gemini-4-hydrogen',
        hex: '56fdd199312815e2',
        mode: variant ? 8 : 1,
        think: 1,
        tool: 0,
        selector: variant || '56fdd199312815e2',
        variant,
      }
      context.geminiChannelGenerate(
        { tokens: { api_key: 'test', bl: 'test' }, auth: {} },
        model,
        context.geminiPayload('1+1=?', model, [], null),
        () => {}
      )
      expect(rpcs).toEqual(['ESY5D', 'VxUbXb', 'aPya6c'])
      expect(
        requests.filter((r) => r.body?.ofs !== undefined).map((r) => [r.body.count, r.body.ofs])
      ).toEqual([
        [1, 0],
        [1, 1],
        [1, 2],
      ])
      expect(requests.at(-1).url.searchParams.get('TYPE')).toBe('terminate')
      expect(host.call).toHaveBeenCalledWith('time.sleep', { milliseconds: 500 })
    }
  )

  it('额度接口明确耗尽时不提交生成，也不编造恢复时间', () => {
    const { context, host } = runtime()
    context.geminiRpc = (_session, rpc) => (rpc === 'aPya6c' ? [true, 1, []] : [])
    const request = vi.fn()
    context.geminiRequest = request
    expect(() =>
      context.geminiChannelGenerate(
        { tokens: { api_key: 'test' } },
        { id: 'gemini-3.8-flash' },
        [],
        () => {}
      )
    ).toThrow('上游未提供恢复时间')
    expect(request).not.toHaveBeenCalled()
    expect(host.checkpoint()).toEqual({ phase: 'ready' })
  })

  it('识别 HTTP 200 内失败标记和静默换模型，不伪装生成成功', () => {
    const { context } = runtime()
    const frame = Array(43).fill(null)
    frame[31] = true
    const raw = () => JSON.stringify([['wrb.fr', null, JSON.stringify(frame)]])
    expect(() => context.geminiParseGeneration(raw(), { name: '4 Argon' })).toThrow('frame[31]')
    frame[31] = false
    frame[42] = '3.5 Flash-Lite'
    expect(() => context.geminiParseGeneration(raw(), { name: '4 Argon' })).toThrow(
      'account_model_unavailable'
    )
    frame[42] = '4 Argon High'
    expect(() => context.geminiParseGeneration(raw(), { name: '4 Argon' })).not.toThrow()
  })

  it('401、验证、限流分别处理，普通 403 不误标登录失效', () => {
    const { context } = runtime()
    expect(() => context.geminiRequireOk({ status: 401 })).toThrow('LOGIN_EXPIRED')
    expect(() => context.geminiRequireOk({ status: 403 })).toThrow('temporary_unavailable')
    expect(() =>
      context.geminiRequireOk({ status: 429, headers: { 'retry-after': '90' } })
    ).toThrow('retry after 90 seconds')
    expect(() =>
      context.geminiRequireOk({ ok: true, final_url: 'https://www.google.com/sorry/index' })
    ).toThrow('verification_required')
  })

  it('恢复只读取指定轮次，拒绝返回别的轮次产物', () => {
    const { context } = runtime()
    context.geminiRpc = () => [[[['c_test', 'r_old'], null, null, []]]]
    expect(
      context.geminiHistory({}, { conversation_id: 'c_test', response_id: 'r_new' })
    ).toBeNull()
  })

  it('下载跨域前校验白名单，不泄露 Cookie 给外部地址', () => {
    const { context, host } = runtime()
    expect(() =>
      context.geminiDownload({ cookies: [] }, 'https://evil.test/image', 'image')
    ).toThrow('下载域名无效')
    expect(host.call).not.toHaveBeenCalled()
    expect(
      context.geminiMediaURLs(
        ['https://lh3.googleusercontent.com/gg/test', 'https://evil.test/test'],
        'image'
      )
    ).toEqual(['https://lh3.googleusercontent.com/gg/test'])
  })

  it('允许实际 Google 图片重定向，但拒绝伪装成 Google 的外部下载域名', () => {
    const { context, host } = runtime()
    host.call
      .mockImplementationOnce(() => ({
        status: 302,
        headers: { location: 'https://lh3.google.com/rd-gg/image' },
      }))
      .mockImplementationOnce(() => ({
        status: 200,
        ok: true,
        body_base64: 'AQID',
        headers: { 'content-type': 'image/jpeg' },
      }))
    expect(
      context.geminiDownload({ cookies: [] }, 'https://lh3.googleusercontent.com/gg/image', 'image')
    ).toBe('data:image/jpeg;base64,AQID')
    expect(() =>
      context.geminiDownload(
        { cookies: [] },
        'https://lh3.google.com.evil.test/rd-gg/image',
        'image'
      )
    ).toThrow('下载域名无效')
  })

  it('不根据未知额度编造恢复时间，视频明确耗尽时立即报错', () => {
    const { context } = runtime()
    expect(() =>
      context.geminiMediaFailure(
        { turn: [null, null, null, ['out of video generation quota']], text: '' },
        'video'
      )
    ).toThrow('上游未提供恢复时间')
  })

  it('用户文字提到额度耗尽，不会污染当前生成的错误判断', () => {
    const { context } = runtime()
    expect(() =>
      context.geminiMediaFailure(
        { turn: [null, 'out of video generation quota', null, []], text: '' },
        'video'
      )
    ).not.toThrow()
  })

  it('视频下载只返回 MP4，跳过同轮的 protobuf 元数据', () => {
    const { context, host } = runtime()
    host.call.mockImplementationOnce(() => ({
      ok: true,
      status: 200,
      body_base64: 'AQID',
      headers: { 'content-type': 'application/protobuf' },
    }))
    expect(
      context.geminiDownload(
        { cookies: [] },
        'https://contribution.usercontent.google.com/download?c=x',
        'video'
      )
    ).toBe('')
    context.geminiHistory = () => ({ turn: [null, null, null, []] })
    context.geminiMediaURLs = () => ['metadata', 'video', 'duplicate']
    context.geminiDownload = vi.fn((_s, url) =>
      url === 'metadata' ? '' : 'data:video/mp4;base64,test'
    )
    expect(context.geminiWaitMedia({}, {}, 'video')).toEqual(['data:video/mp4;base64,test'])
    expect(context.geminiDownload).toHaveBeenCalledTimes(2)
  })

  it('已明确失败的任务不会从历史中读取拒绝文案伪装成成功', () => {
    const { context, host } = runtime()
    context.geminiChannelGenerate = (_s, _m, _i, callback) => {
      const frame = Array(43).fill(null)
      frame[31] = true
      callback(frame)
    }
    expect(() =>
      context.geminiGenerate({}, { mode: 1, think: 1, tool: 0 }, 'test', [], null)
    ).toThrow('frame[31]')
    expect(host.checkpoint().phase).toBe('failed')
    context.geminiInitialize = vi.fn()
    expect(() => context.main({ request: { model: 'gemini-3.8-flash', input: 'test' } })).toThrow(
      'frame[31]'
    )
    expect(context.geminiInitialize).not.toHaveBeenCalled()
  })

  it('文本成功后保存结果，恢复同一个请求不再次消耗额度', () => {
    const { context, host } = runtime()
    context.apiSessionRead = () => ({})
    context.geminiInitialize = (session) => session
    context.geminiModel = () => ({
      id: 'gemini-3.8-flash',
      name: '3.8 Flash',
      mode: 1,
      think: 1,
      tool: 0,
    })
    context.geminiReferences = () => []
    const generate = vi.fn((_session, _model, _inner, onFrame) => {
      const frame = Array(43).fill(null)
      frame[1] = ['c_current', 'r_current']
      frame[4] = [['rc_current', ['2']]]
      frame[42] = '3.8 Flash'
      onFrame(frame)
    })
    context.geminiChannelGenerate = generate
    const input = { request: { input: '一加一', model: 'gemini-3.8-flash' } }
    const result = context.main(input)
    expect(result.response.output_text).toBe('2')
    expect(host.checkpoint().phase).toBe('completed')
    expect(context.main(input)).toEqual(result)
    expect(generate).toHaveBeenCalledTimes(1)
  })

  it.each(['generate_image', 'generate_video'])('%s 断点恢复只取原任务产物', (flow) => {
    const { context, host } = runtime(flow)
    const kind = flow === 'generate_video' ? 'video' : 'image'
    host.checkpoint({ phase: 'accepted', conversation_id: 'c_original', response_id: 'r_original' })
    context.apiSessionRead = () => ({})
    context.geminiInitialize = (session) => session
    context.geminiGenerate = vi.fn()
    context.geminiWaitMedia = vi.fn(() => ['data:' + kind + '/test;base64,AAAA'])
    const result = context.main({ prompt: '测试', model_id: 'gemini-' + kind })
    expect(result.success).toBe(true)
    expect(context.geminiGenerate).not.toHaveBeenCalled()
    expect(context.geminiWaitMedia).toHaveBeenCalledWith(
      {},
      expect.objectContaining({ conversation_id: 'c_original', response_id: 'r_original' }),
      kind
    )
  })
})
