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
    expect(
      context.geminiParseGeneration(
        JSON.stringify([['wrb.fr', null, JSON.stringify(frame)]]),
        model
      ).actual_model_name
    ).toBe('4 Carbon')
    data[24][1] = []
    expect(context.geminiCatalog({})).toEqual([])
  })

  it('旧名 Argon 只路由到账号开放的同编号无变体 Carbon', () => {
    const { context } = runtime()
    const row = Array(18).fill(null)
    row[0] = '95a4c0abcba24364'
    row[12] = '4 Carbon'
    row[17] = 8
    const choice = ['', 8, false, null, 'Carbon', '4 Carbon']
    const data = Array(25).fill(null)
    data[15] = [row]
    data[24] = ['neon_launch_ultra_policy', [[choice, [[1]]]]]
    context.geminiRpc = () => data
    expect(context.geminiModel({}, 'gemini-4-argon')).toMatchObject({
      id: 'gemini-4-carbon',
      mode: 8,
      selector: row[0],
    })
    choice[3] = 'carbon'
    expect(() => context.geminiModel({}, 'gemini-4-argon')).toThrow('account_model_unavailable')
    data[24][1] = []
    expect(() => context.geminiModel({}, 'gemini-4-argon')).toThrow('account_model_unavailable')
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

  it('解析独立阶段提示帧，不将会话 token 或其他字段当作思考内容', () => {
    const { context } = runtime()
    const encode = context.geminiWireEncode
    let encoded = encode([
      [7, encode([[6, encode([[1, 'Calculating the requested numerical sum']])]])],
      [27, [255, 0, 128]],
      [43, '3.1 Pro High'],
    ])
    for (const field of [4, 4, 1, 7, 2]) encoded = encode([[field, encoded]])
    const { frame } = context.geminiWireEvent([context.geminiWireBase64(encoded)])
    const result = context.geminiParseGeneration(
      JSON.stringify([['wrb.fr', null, JSON.stringify(frame)]]),
      {}
    )
    expect(result.reasoning_status).toBe('Calculating the requested numerical sum')
    expect(result.reasoning).toBe('')
    expect(result.text).toBe('')
    expect(result.actual_model_name).toBe('3.1 Pro High')
  })

  it('确认受理即发出处理中事件，阶段提示和摘要在正文前实时推送且不重复', () => {
    const { context, host } = runtime()
    const events = []
    host.emit.mockImplementation((event) => events.push(JSON.parse(JSON.stringify(event))))
    context.apiSessionRead = () => ({})
    context.geminiInitialize = (session) => session
    context.geminiModel = () => ({ id: 'gemini-3.1-pro', name: '3.1 Pro', tool: 0 })
    context.geminiReferences = () => []
    context.geminiChannelGenerate = (_session, _model, _payload, onFrame) => {
      const receipt = Array(43).fill(null)
      receipt[1] = ['c_current', 'r_current']
      onFrame(receipt)
      expect(events.map((event) => event.type)).toEqual([
        'response.created',
        'response.in_progress',
      ])
      for (const title of ['计算数值总和', '计算数值总和', '核验计算结果']) {
        const frame = Array(43).fill(null)
        frame[6] = [null, null, null, null, null, [title]]
        onFrame(frame)
        expect(events.at(-1).item.type).toBe('reasoning')
      }
      const candidate = Array(38).fill(null)
      candidate[0] = 'rc_current'
      candidate[1] = ['']
      candidate[37] = [['核对整除条件与求和范围。']]
      const frame = Array(43).fill(null)
      frame[4] = [candidate]
      onFrame(frame)
      expect(events.filter((event) => event.type === 'response.output_text.delta')).toEqual([])
      expect(events.some((event) => event.delta === '核对整除条件与求和范围。')).toBe(true)
      candidate[1] = ['201003']
      onFrame(frame)
    }
    const result = context.main({ request: { model: 'gemini-3.1-pro', input: '计算并核验' } })
    const thoughts = events.filter(
      (event) => event.type === 'response.reasoning_summary_text.delta'
    )
    expect(thoughts.map((event) => event.delta)).toEqual([
      '计算数值总和',
      '\n核验计算结果',
      '核对整除条件与求和范围。',
    ])
    expect(result.response.output_text).toBe('201003')
    expect(result.response.output.filter((item) => item.type === 'reasoning')).toHaveLength(2)
    expect(result.response.output.every((item) => item.status === 'completed')).toBe(true)
  })

  it('已取得轮次回执后读取通道的 HTTP 400 进入历史恢复，提交参数错误保持原分类', () => {
    const { context, host } = runtime()
    const response = { ok: false, status: 400 }
    expect(() => context.geminiRequirePollOk(response)).toThrow('invalid_input')
    host.checkpoint({ phase: 'accepted', conversation_id: 'c_real', response_id: 'r_real' })
    expect(() => context.geminiRequirePollOk(response)).toThrow('temporary_unavailable')
    expect(() => context.geminiRequireOk(response)).toThrow('invalid_input')
  })

  it('确认受理前额度错误不提前输出，保留现有自动换号机制', () => {
    const { context, host } = runtime()
    context.apiSessionRead = () => ({})
    context.geminiInitialize = (session) => session
    context.geminiModel = () => ({ id: 'gemini-3.1-pro', tool: 0 })
    context.geminiReferences = () => []
    context.geminiChannelGenerate = () => {
      throw new Error('quota_exhausted: unavailable')
    }
    expect(() => context.main({ request: { model: 'gemini-3.1-pro', input: '测试' } })).toThrow(
      'quota_exhausted'
    )
    expect(host.emit).not.toHaveBeenCalled()
  })

  it('跨分块编码长载荷时保留所有字节，包括零字节和高位字节', () => {
    const { context } = runtime()
    const bytes = Array.from({ length: 300001 }, (_, i) => i % 256)
    expect(context.geminiWireBase64(bytes)).toBe(Buffer.from(bytes).toString('base64'))
    expect(context.geminiWireBase64([])).toBe('')
  })

  it('生成请求的第 31 字段使用网页的 packed enum 编码，不嵌套消息', () => {
    const { context } = runtime()
    const payload = context.geminiPayload('', { mode: 1, think: 1, tool: 0 }, [], null)
    const fields = context.geminiWireDecode(context.geminiWireInner(payload))
    // 来自成功网页请求：tag=31/wire=2，长度=4，值为 4、16、19、12。
    expect(fields[31]).toEqual([[4, 16, 19, 12]])
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

  it.each([true, false])('长历史被明确拒绝后只换一次承载方式，第二次成功=%s', (succeeds) => {
    const { context, host } = runtime()
    context.apiSessionRead = () => ({})
    context.geminiInitialize = (session) => session
    context.geminiModel = () => ({ id: 'gemini-4-hydrogen' })
    const original = [['original-reference']]
    context.geminiReferences = vi.fn(() => original)
    context.geminiUpload = vi.fn((_s, file) => {
      expect(Buffer.from(file.base64, 'base64').toString()).toBe(
        '[SYSTEM]\n指令\n\n[USER]\n先前问题\n\n[ASSISTANT]\n先前回答'
      )
      return ['history-reference']
    })
    const reject = () => {
      host.checkpoint({ ...host.checkpoint(), phase: 'ready', rejected_message_bytes: 2000000 })
      throw new Error('invalid_input: rejected')
    }
    context.geminiGenerate = vi
      .fn()
      .mockImplementationOnce(reject)
      .mockImplementation((_s, _m, prompt, files) => {
        expect(prompt).toContain('# INSTRUCTIONS\n本次指令')
        expect(prompt).toContain('[USER]\n继续')
        expect(files).toEqual([['history-reference'], ...original])
        if (!succeeds) reject()
        return { text: '完成' }
      })
    const input = {
      request: {
        model: 'gemini-4-hydrogen',
        instructions: '本次指令',
        input: [
          ['system', '指令'],
          ['user', '先前问题'],
          ['assistant', '先前回答'],
          ['user', '继续'],
        ].map(([role, content]) => ({ role, content })),
      },
    }
    if (succeeds) expect(context.main(input).response.output_text).toBe('完成')
    else expect(() => context.main(input)).toThrow('invalid_input')
    expect(context.geminiGenerate).toHaveBeenCalledTimes(2)
    expect(context.geminiUpload).toHaveBeenCalledTimes(1)
    expect(context.geminiReferences).toHaveBeenCalledTimes(1)
  })

  it('识别 HTTP 200 内失败标记，并保留上游返回的实际模型', () => {
    const { context } = runtime()
    const frame = Array(43).fill(null)
    frame[31] = true
    const raw = () => JSON.stringify([['wrb.fr', null, JSON.stringify(frame)]])
    expect(() => context.geminiParseGeneration(raw(), { name: '4 Argon' })).toThrow('frame[31]')
    frame[31] = false
    frame[42] = '3.5 Flash-Lite'
    expect(context.geminiParseGeneration(raw(), { name: '4 Argon' }).actual_model_name).toBe(
      '3.5 Flash-Lite'
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

  it('识别真实 RPC 空结果中的错误码并终止无法加载的会话', () => {
    const { context, host } = runtime()
    context.geminiRequest = () => ({
      body_text: JSON.stringify([
        [
          'wrb.fr',
          'hNvQHb',
          null,
          null,
          null,
          [
            5,
            null,
            [
              [
                'type.googleapis.com/assistant.boq.bard.application.BardErrorInfo',
                [1167, null, '无法加载对话。（1167）'],
              ],
            ],
          ],
          'generic',
        ],
      ]),
    })
    expect(() =>
      context.geminiHistory(
        { auth: {}, tokens: {} },
        { conversation_id: 'c_original', response_id: 'r_original' }
      )
    ).toThrow('conversation_unavailable')
    host.checkpoint({ phase: 'accepted', conversation_id: 'c_original', response_id: 'r_original' })
    context.apiSessionRead = () => ({ auth: {}, tokens: {} })
    const initialize = vi.fn((session) => session)
    context.geminiInitialize = initialize
    const input = { request: { model: 'gemini-4-argon', input: 'test' } }
    expect(() => context.main(input)).toThrow('conversation_unavailable')
    expect(host.checkpoint().phase).toBe('failed')
    expect(() => context.main(input)).toThrow('conversation_unavailable')
    expect(initialize).toHaveBeenCalledTimes(1)
  })

  it.each(['通道关闭', '读取断线', '读取响应体中断', '通道正常结束但无正文'])(
    '%s 后反复刷新同一轮历史，接回迟到的完整正文',
    (mode) => {
      const { context, host } = runtime()
      context.apiSessionRead = () => ({})
      context.geminiInitialize = (s) => s
      context.geminiModel = () => ({ id: 'gemini-4-hydrogen' })
      context.geminiReferences = () => []
      context.geminiGenerate = vi.fn((_s, _m, _p, _f, _prior, _ratio, emit) => {
        host.checkpoint({
          phase: 'accepted',
          conversation_id: 'c_test',
          response_id: 'r_test',
          received_text_length: 2,
          channel_ended: mode === '通道关闭' || mode === '通道正常结束但无正文',
        })
        if (mode === '通道正常结束但无正文') return { text: '' }
        emit({ text: '原文' })
        throw new Error(
          mode === '通道关闭'
            ? 'temporary_unavailable: channel ended'
            : mode === '读取响应体中断'
              ? 'error decoding response body: error reading a body from connection'
              : 'unexpected EOF'
        )
      })
      context.geminiHistory = vi
        .fn()
        .mockReturnValueOnce(null)
        .mockImplementationOnce(() => {
          throw new Error('temporary_unavailable: Gemini HTTP 503')
        })
        .mockReturnValueOnce({ text: '短' })
        .mockReturnValueOnce({ text: '原文和后续。' })
      const req = { request: { model: 'gemini-4-hydrogen', input: 'test' } }
      expect(context.main(req).response.output_text).toBe('原文和后续。')
      expect(context.main(req).response.output_text).toBe('原文和后续。')
      expect(context.geminiGenerate).toHaveBeenCalledTimes(1)
      expect(context.geminiHistory.mock.calls.map((args) => args[1].response_id)).toEqual(
        Array(4).fill('r_test')
      )
      expect(
        host.emit.mock.calls
          .map(([e]) => (e.type === 'response.output_text.delta' ? e.delta : ''))
          .join('')
      ).toBe('原文和后续。')
    }
  )

  it('降级后的通道仍没有结果时终止，不循环降级', () => {
    const { context, host } = runtime()
    host.checkpoint({
      phase: 'accepted',
      conversation_id: 'c_original',
      response_id: 'r_original',
      fallback_model: 'gemini-4-hydrogen',
      channel_ended: true,
    })
    context.apiSessionRead = () => ({})
    context.geminiInitialize = (session) => session
    context.geminiHistory = vi.fn(() => null)
    const input = { request: { model: 'gemini-4-argon', input: 'test' } }
    expect(() => context.main(input)).toThrow('generation_failed')
    expect(() => context.main(input)).toThrow('generation_failed')
    expect(context.geminiHistory).toHaveBeenCalledTimes(8)
    expect(host.checkpoint()).toMatchObject({
      phase: 'failed',
      history_lookup: { attempt: 8, found_turn: false, history_chars: 0 },
    })
  })

  it('原通道确定结束且没有正文时按账号目录降级一次，通过标准 model 返回实际模型', () => {
    const { context, host } = runtime()
    host.checkpoint({
      phase: 'accepted',
      conversation_id: 'c_original',
      response_id: 'r_original',
      channel_ended: true,
    })
    context.apiSessionRead = () => ({})
    context.geminiInitialize = (session) => session
    context.geminiHistory = vi.fn(() => null)
    context.geminiCatalog = () => [{ id: 'gemini-4-hydrogen', name: '4 Hydrogen', mode: 1 }]
    context.geminiReferences = () => []
    context.geminiGenerate = vi.fn((_session, model) => {
      expect(model.id).toBe('gemini-4-hydrogen')
      expect(host.checkpoint()).toMatchObject({
        phase: 'ready',
        fallback_model: 'gemini-4-hydrogen',
        original_receipt: { conversation_id: 'c_original', response_id: 'r_original' },
      })
      return { conversation_id: 'c_fallback', response_id: 'r_fallback', text: 'OK' }
    })
    const input = { request: { model: 'gemini-4-argon', input: 'test' } }
    const response = context.main(input).response
    expect(response.model).toBe('gemini-4-hydrogen')
    expect(response.output_text).toBe('OK')
    expect(context.main(input).response.output_text).toBe(response.output_text)
    expect(context.geminiGenerate).toHaveBeenCalledTimes(1)
  })

  it.each([true, false])('基础模型通道关闭无正文时仅补发一次，第二次成功=%s', (success) => {
    const { context, host } = runtime()
    const model = { id: 'gemini-4-hydrogen', name: '4 Hydrogen', think: 4 }
    context.apiSessionRead = () => ({})
    context.geminiInitialize = (s) => s
    context.geminiModel = () => model
    context.geminiCatalog = () => [{ ...model, mode: 1 }]
    context.geminiHistory = () => null
    context.geminiReferences = vi.fn(() => [])
    let calls = 0
    context.geminiGenerate = (_s, selected) => {
      expect(selected).toEqual(model)
      calls++
      if (calls === 2 && success)
        return { text: '正常回答', conversation_id: 'c_ok', response_id: 'r_ok' }
      host.checkpoint({
        ...host.checkpoint(),
        phase: 'accepted',
        conversation_id: 'c_empty',
        response_id: 'r_empty',
        channel_ended: true,
      })
      throw new Error('temporary_unavailable: channel ended')
    }
    const req = { request: { model: model.id, input: 'test' } }
    for (let i = 0; i < 2; i++) {
      if (success) expect(context.main(req).response.output_text).toBe('正常回答')
      else expect(() => context.main(req)).toThrow('incomplete_upstream_turn')
    }
    expect(calls).toBe(2)
    expect(context.geminiReferences).toHaveBeenCalledTimes(1)
  })

  it('恢复等待有跨请求时限，过期后不再查询上游或重发生成', () => {
    const { context, host } = runtime()
    host.checkpoint({
      phase: 'accepted',
      conversation_id: 'c_original',
      response_id: 'r_original',
      accepted_at: Date.now() - 21 * 60 * 1000,
    })
    context.apiSessionRead = () => ({})
    context.geminiInitialize = (session) => session
    context.geminiHistory = vi.fn()
    context.geminiGenerate = vi.fn()
    const input = { request: { model: 'gemini-4-argon', input: 'test' } }
    expect(() => context.main(input)).toThrow('recovery_exhausted')
    expect(() => context.main(input)).toThrow('recovery_exhausted')
    expect(context.geminiHistory).not.toHaveBeenCalled()
    expect(context.geminiGenerate).not.toHaveBeenCalled()
    expect(host.checkpoint().phase).toBe('failed')
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
      frame[4] = [['rc_failed', ['我只是一个语言模型，不具备这方面的信息或能力，因此没法帮到你。']]]
      frame[42] = '3.5 Flash-Lite'
      frame[26] = 'private-session-token'
      callback(frame)
    }
    expect(() =>
      context.geminiGenerate({}, { mode: 1, think: 1, tool: 0 }, 'test', [], null)
    ).toThrow('frame[31]')
    expect(host.checkpoint().phase).toBe('failed')
    expect(host.checkpoint().failure_frame[26]).toBe('private-session-token')
    expect(host.checkpoint().error).toContain('不具备这方面的信息或能力')
    expect(host.checkpoint().error).not.toContain('private-session-token')
    expect(host.checkpoint().failure_context).toMatchObject({
      prompt_chars: 4,
      attachment_count: 0,
      continued: false,
    })
    context.geminiInitialize = vi.fn()
    expect(() => context.main({ request: { model: 'gemini-3.8-flash', input: 'test' } })).toThrow(
      'frame[31]'
    )
    expect(context.geminiInitialize).not.toHaveBeenCalled()
  })

  it('空失败帧只查询当前轮次的拒绝原因，查询失败仍保持终态且不重新生成', () => {
    for (const lookupFails of [false, true]) {
      const { context, host } = runtime()
      const turn = [['c_current', 'r_current'], null, null, [[['rc', ['上游拒绝原因']]]]]
      context.geminiRpc = vi.fn(() => {
        if (lookupFails) throw new Error('lookup timeout')
        return [
          [[['c_current', 'r_old'], null, null, [[['rc_old', ['旧回答不能串到本轮']]]]], turn],
        ]
      })
      context.geminiChannelGenerate = vi.fn((_s, _m, _i, callback) => {
        const frame = Array(43).fill(null)
        frame[1] = ['c_current', 'r_current']
        frame[31] = true
        callback(frame)
      })
      expect(() => context.geminiGenerate({}, { mode: 1 }, 'test', [], null)).toThrow(
        lookupFails ? 'upstream_generation_failed' : '上游拒绝原因'
      )
      expect(host.checkpoint().phase).toBe('failed')
      expect(host.checkpoint().error).not.toContain('旧回答')
      expect(context.geminiRpc).toHaveBeenCalledTimes(1)
      expect(context.geminiChannelGenerate).toHaveBeenCalledTimes(1)
      if (lookupFails) expect(host.checkpoint().failure_lookup_error).toBe('lookup timeout')
      else expect(host.checkpoint().failure_history).toEqual(turn)
    }
  })

  it('上传只重试尚未建立连接的步骤，额度和提交后的错误直接返回', () => {
    const { context } = runtime()
    for (const message of [
      'client error (Connect); caused by: unexpected EOF',
      'rate_limit_exceeded: HTTP 429',
      'connection reset after write',
    ]) {
      context.geminiRequest = vi
        .fn()
        .mockImplementationOnce(() => {
          throw new Error(message)
        })
        .mockReturnValueOnce({ ok: true })
      if (message.includes('(Connect)')) {
        expect(context.geminiUploadRequest({}, 'upload', {})).toEqual({ ok: true })
        expect(context.geminiRequest).toHaveBeenCalledTimes(2)
      } else {
        expect(() => context.geminiUploadRequest({}, 'upload', {})).toThrow(message)
        expect(context.geminiRequest).toHaveBeenCalledTimes(1)
      }
    }
  })

  it('超出上游附件数量时返回实际数量，不启动文件上传', () => {
    const { context } = runtime()
    context.geminiUpload = vi.fn()
    const attachments = Array.from({ length: 15 }, (_, index) => ({ name: `${index}.txt` }))
    expect(() => context.geminiReferences({}, {}, { input: [{ attachments }] })).toThrow(
      'attachment_limit_exceeded: {"count":15,"limit":10}'
    )
    expect(context.geminiUpload).not.toHaveBeenCalled()
  })

  it('保留失败响应的原始二进制帧用于加密诊断，不把拒绝正文当作成功答案', () => {
    const { context } = runtime()
    let bytes = context.geminiWireEncode([[32, 1]])
    for (const field of [4, 4, 1, 7, 2]) bytes = context.geminiWireEncode([[field, bytes]])
    const raw = context.geminiWireBase64(bytes)
    const decoded = context.geminiWireEvent([raw])
    expect(decoded.failure_wire).toBe(raw)
    expect(decoded.frame[31]).toBe(true)
    expect(() =>
      context.geminiParseGeneration(
        JSON.stringify([['wrb.fr', null, JSON.stringify(decoded.frame)]]),
        {}
      )
    ).toThrow('upstream_generation_failed')
  })

  it('采用上游实际降级结果，正文保持原样，流式和结果都返回真实模型', () => {
    const { context, host } = runtime()
    context.apiSessionRead = () => ({})
    context.geminiInitialize = (session) => session
    context.geminiModel = () => ({
      id: 'gemini-4-argon',
      name: '4 Argon',
      mode: 8,
      think: 1,
      tool: 0,
    })
    context.geminiReferences = () => []
    context.geminiChannelGenerate = (_s, _m, _p, callback) => {
      for (const text of ['OK', 'OK done']) {
        const frame = Array(43).fill(null)
        frame[1] = ['c_current', 'r_current']
        frame[4] = [['rc_current', [text]]]
        frame[42] = '4 Hydrogen'
        callback(frame)
      }
    }
    const input = {
      request: { input: 'test', model: 'gemini-4-argon', options: { session_key: 'test' } },
    }
    const output = context.main(input).response
    expect(output.model).toBe('gemini-4-hydrogen')
    expect(output.metadata.requested_model).toBeUndefined()
    expect(output.metadata.actual_model).toBeUndefined()
    expect(output.metadata.actual_model_name).toBeUndefined()
    expect(output.metadata.model_notice).toBeUndefined()
    expect(output.metadata.platform_session.conversation_id).toBe('c_current')
    expect(output.output_text).toBe('OK done')
    expect(
      host.emit.mock.calls
        .map(([event]) => (event.type === 'response.output_text.delta' ? event.delta : ''))
        .join('')
    ).toBe(output.output_text)
    expect(context.main(input).response.output_text).toBe(output.output_text)
  })

  it.each([
    '正常文案',
    '失败后恢复',
    '再次失败',
    '正文已输出',
    '思考已输出',
    '思考进度已输出',
    '二次请求报错',
  ])('只按真实失败标记和已输出内容决定是否重试：%s', (scenario) => {
    const { context, host } = runtime()
    const normal = '我无法提供这方面的帮助，因为我只是一个语言模型。'
    const detail = 'Service failed.\n上游返回的完整原因。'
    const input = { request: { model: 'gemini-4-hydrogen', input: '测试' } }
    context.apiSessionRead = () => ({})
    context.geminiInitialize = (session) => session
    context.geminiModel = () => ({ id: input.request.model, mode: 1, think: 1, tool: 0 })
    context.geminiReferences = vi.fn(() => [])
    context.geminiHistory = () => null
    let attempts = 0
    context.geminiChannelGenerate = (_s, _m, _p, callback) => {
      attempts++
      const send = (text, marker, reasoning, progress) => {
        const frame = Array(43).fill(null)
        frame[1] = ['c_test', 'r_' + attempts]
        const candidate = ['rc', [text]]
        if (reasoning) candidate[37] = [[reasoning]]
        if (progress) frame[6] = [null, null, null, null, null, [progress]]
        frame[4] = [candidate]
        frame[31] = marker
        try {
          callback(frame)
        } catch (error) {
          throw new Error('事件流回调异常：' + error.message)
        }
      }
      if (scenario === '正常文案') return send(normal, false)
      if (attempts === 2) {
        if (scenario === '二次请求报错') throw new Error('invalid_input: 二次请求参数错误')
        return send(scenario === '再次失败' ? detail : normal, scenario === '再次失败')
      }
      send(
        scenario === '正文已输出' ? '第一段' : '',
        false,
        scenario === '思考已输出' ? '正在核对文件内容' : '',
        scenario === '思考进度已输出' ? '正在读取文件' : ''
      )
      send(scenario.endsWith('已输出') ? detail : '', 1)
    }
    for (let i = 0; i < 2; i++) {
      if (scenario === '正常文案' || scenario === '失败后恢复')
        expect(context.main(input).response.output_text).toBe(normal)
      else if (scenario === '二次请求报错')
        expect(() => context.main(input)).toThrow('二次请求参数错误')
      else {
        let error
        try {
          context.main(input)
        } catch (e) {
          error = e
        }
        expect(context.geminiFailureReply(error)).toBe(detail)
      }
    }
    expect(attempts).toBe(scenario === '正常文案' || scenario.endsWith('已输出') ? 1 : 2)
    expect(context.geminiReferences).toHaveBeenCalledTimes(1)
    const deltas = host.emit.mock.calls
      .map(([e]) => (e.type === 'response.output_text.delta' ? e.delta : ''))
      .join('')
    expect(deltas).toBe(
      scenario === '正常文案' || scenario === '失败后恢复'
        ? normal
        : scenario === '正文已输出'
          ? '第一段'
          : ''
    )
  })

  it('重启回读保留响应身份，等待正确轮次，且恢复脚本没有生成入口', () => {
    const { context, host } = runtime('recover_text_receipt')
    expect(context.geminiGenerate).toBeUndefined()
    expect(context.geminiUpload).toBeUndefined()
    context.apiSessionRead = () => ({})
    context.geminiInitialize = (s) => s
    expect(manifest.capabilities.context_fields).toContain('task_id')
    const original = runtime()
    original.context.apiSessionRead = () => ({})
    original.context.geminiInitialize = (s) => s
    original.context.geminiModel = () => ({ id: 'gemini-4-hydrogen', mode: 1 })
    original.context.geminiChannelGenerate = (_s, _m, _p, onFrame) => {
      const frame = Array(43).fill(null)
      frame[1] = ['c_exact', 'r_exact']
      frame[4] = [['rc_exact', ['最终回']]]
      onFrame(frame)
      throw new Error('进程退出')
    }
    expect(() =>
      original.context.main({
        task_id: 'receipt_test',
        request: { model: 'gemini-4-hydrogen', input: '用户原始输入', stream: true },
      })
    ).toThrow('进程退出')
    const saved = original.host.checkpoint()
    const created = original.host.emit.mock.calls.find(([e]) => e.type === 'response.created')[0]
      .response
    expect(saved.phase).toBe('accepted')
    expect(JSON.stringify(saved.receipt_writer)).not.toContain('用户原始输入')
    host.checkpoint(saved)
    context.geminiRpc = vi.fn(() => [
      [[['c_exact', 'r_other'], null, null, [[['rc_other', ['其他轮次']]]]]],
    ])
    expect(() => context.main({})).toThrow('receipt_pending')
    context.geminiRpc = () => [
      [[['c_exact', 'r_exact'], null, null, [[['rc_exact', ['最终回答。']]]]]],
    ]
    const response = context.main({}).response
    expect(response).toMatchObject({
      id: created.id,
      created_at: created.created_at,
      status: 'completed',
      output_text: '最终回答。',
    })
    host.checkpoint({ phase: 'submitting' })
    expect(() => context.main({})).toThrow('receipt_terminal')
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
