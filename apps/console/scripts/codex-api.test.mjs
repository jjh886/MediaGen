import { describe, expect, it } from 'vitest'
import codex from '../../../plugins/codex/manifest.json'
import { scripts } from './platform-api-test-helper.mjs'

describe('Codex GPT-6.2', () => {
  const models = ['gpt-6.2-sol', 'gpt-6.2-astra'].map((slug) => ({
    slug,
    service_tiers: [{ id: 'priority' }],
    supported_reasoning_levels: [
      'none',
      'minimal',
      'low',
      'medium',
      'high',
      'xhigh',
      'max',
      'ultra',
    ].map((effort) => ({ effort })),
    default_reasoning_level: 'low',
  }))
  it.each([
    ['pro', true, false],
    ['chatgpt_pro', true, false],
    ['Pro Max', true, true],
    ['prolite', false, false],
    ['enterprise', false, true],
  ])('按 %s 账号目录校验服务档位和思考深度', (tier, sol, astra) => {
    const visible = structuredClone(models)
    if (tier === 'enterprise') visible[1].service_tiers.push({ id: 'ultrafast' })
    const catalog = (request) => {
      expect(request.url).toContain('client_version=0.169.0')
      expect(request.headers.version).toBe('0.169.0')
      return { ok: true, body: { models: visible } }
    }
    const run = scripts(
      [
        'codex/scripts/auth.js',
        'codex/scripts/request_input.js',
        'codex/scripts/text_responses.js',
        'codex/scripts/list_models.js',
      ],
      {
        call: (_, request) => catalog(request),
        tryCall: (_, request) => ({ success: true, data: catalog(request) }),
      }
    )
    run('codexEnsureFreshAuth = () => ({})')
    const discovered = run('main(ctx).models', { ctx: { account: { tier } } })
    expect(discovered).toHaveLength(2)
    for (const [i, model] of discovered.entries()) {
      const entry = codex.models.find((item) => item.model_id === model.model_id)
      expect(entry.aliases).toContain(models[i].slug)
      expect(entry.capabilities.reasoning).toEqual(model.capabilities.reasoning)
      expect(entry.capabilities.context_window).toBe(i === 0 ? 500000 : 272000)
      expect(entry.capabilities.max_context_window).toBe(872000)
      expect(entry.capabilities.pricing.input).toBe(i === 0 ? 2 : 8)
      expect(
        codex.generation_settings.fields[0].options.some(
          (option) => option.value === models[i].slug
        )
      ).toBe(true)
      const payload = run('codexNormalizeResponsesRequest(request, {}).payload', {
        request: {
          model: model.model_id,
          input: '你好',
          include: ['message.output_text.logprobs', 'reasoning.encrypted_content'],
        },
      })
      expect(payload.model).toBe(models[i].slug)
      expect(payload.include).toEqual(['reasoning.encrypted_content'])
      run('codexApplyReasoning(ctx, {}, payload)', { payload })
      expect(payload.reasoning.effort).toBe('low')
      for (const effort of ['none', 'minimal']) {
        expect(() =>
          run('codexApplyReasoning(ctx, {}, {...payload, reasoning:{effort}})', { effort })
        ).toThrow('invalid_input')
      }
      const allowed = i === 0 ? sol : astra
      expect(model.capabilities.service_tiers.includes('ultrafast')).toBe(allowed)
      const request = () =>
        run(
          "codexApplyReasoning(ctx, {}, {...payload, reasoning:{effort:'ultra'}, service_tier:'ultrafast'})"
        )
      if (allowed) expect(request).not.toThrow()
      else expect(request).toThrow('ultrafast')
    }
  })
})

it('Codex 声明 ID 与引用同步，桌面上下文以用户消息发送', () => {
  const run = scripts(['codex/scripts/request_input.js'])
  const id = 'at_12345678-1234-5678-abcd-123456789abc'
  const output = {
    type: 'function_call_output',
    namespace: 'agent',
    name: 'browser_info',
    output: '外部内容',
  }
  const page = { ...output, name: 'codex_apps_open_page', output: '{"page_id":"page"}' }
  const input = [
    { type: 'item_reference', id },
    { type: 'additional_tools', id },
    { type: 'function_call', id },
    output,
    { ...output, call_id: 'call_1' },
    page,
    { ...page, output: '{}' },
  ]
  const items = run('codexNormalizeResponsesInput(input)', { input })
  expect(items.map((item) => item.id).slice(0, 3)).toEqual([
    id.replaceAll('-', ''),
    id.replaceAll('-', ''),
    id,
  ])
  expect(items[3]).toMatchObject({
    role: 'user',
    content: [{ text: '<external_browser_info>外部内容</external_browser_info>' }],
  })
  expect(items[4]).toEqual(input[4])
  expect(items[5].role).toBe('user')
  expect(items[6]).toEqual(input[6])
})

it('Codex 只清理工具 schema 中不兼容的正则，保留普通数据和有效约束', () => {
  const run = scripts(['codex/scripts/request_input.js'])
  const parameters = {
    properties: { value: { pattern: '(?=bad)' }, code: { pattern: '(?P<word>ok)' } },
    patternProperties: { '\\p{L}': { pattern: '(?<=bad)' }, '^ok': { type: 'string' } },
    additionalProperties: { type: 'number' },
    examples: [{ pattern: '(?=literal)' }],
  }
  const tool = { type: 'function', name: 'test', parameters }
  const result = run(
    "codexNormalizeResponsesInput([{type:'additional_tools',tools:[tool]}])[0].tools[0].parameters",
    { tool }
  )
  expect(result).toEqual({
    properties: { value: {}, code: { pattern: '(?P<word>ok)' } },
    patternProperties: { '^ok': { type: 'string' } },
    additionalProperties: true,
    examples: [{ pattern: '(?=literal)' }],
  })
})

it('Codex 流式失败保留错误码，并排除响应正文', () => {
  for (const type of ['error', 'response.failed']) {
    const error = { code: 'insufficient_quota', message: '账户余额不足', resets_in_seconds: 30 }
    const run = scripts(['codex/scripts/auth.js', 'codex/scripts/text_responses.js'], {
      stream: (_a, _r, emit) => {
        emit(
          type === 'error'
            ? { type, ...error }
            : { type, response: { error, output_text: '私有正文' } }
        )
        return { ok: true, status: 200 }
      },
    })
    const result = run('codexExecuteResponses({}, {})')
    expect(result.error).toContain('quota_exhausted:')
    expect(result.error).toContain('retry after 30 seconds')
    expect(result.error).not.toContain('私有正文')
  }
})

it('工具结果缺失时区分输入错误和连接失败', () => {
  const run = scripts(['codex/scripts/text_responses.js'])
  expect(
    run(
      "codexResponsesHttpError({body:{message:'No tool output found for function call call_test.'}})"
    )
  ).toBe('invalid_input: {"reason":"missing_tool_output"}')
})

it.each([false, true])('流在首段内容前被拒绝才能安全换号，已输出=%s', (started) => {
  const emitted = []
  const checkpoints = []
  const flow = codex.flows.generate_text.steps[0].script_files.map(
    (name) => 'codex/scripts/' + name
  )
  const run = scripts(flow, {
    call: () => ({ tokens: { access_token: 'test' } }),
    tryCall: () => ({ success: false }),
    checkpoint: (value) => {
      if (value) checkpoints.push(value.phase)
    },
    emit: (event) => emitted.push(event.type),
    stream: (_action, _request, consume) => {
      const events = [{ type: 'response.created' }, { type: 'codex.rate_limits' }]
      if (started) events.push({ type: 'response.output_text.delta', delta: '内容' })
      events.push({
        type: 'response.failed',
        response: { error: { code: 'usage_limit_reached', message: '额度用尽' } },
      })
      for (const event of events) {
        const mapped = consume(event)
        if (mapped) emitted.push(mapped.type)
      }
      return { ok: true, status: 200 }
    },
  })
  expect(() =>
    run("main({profile_dir:'.',request:{model:'codex-gpt-6.2-sol',input:'你好',stream:true}})")
  ).toThrow('quota_exhausted')
  expect(emitted).toEqual(
    started ? ['response.created', 'codex.rate_limits', 'response.output_text.delta'] : []
  )
  expect(checkpoints).toEqual(started ? ['submitting'] : ['submitting', 'ready'])
})

it.each([
  ['generate_text', 'sol'],
  ['compact_text', 'astra'],
])('%s 以正式模型 ID 完成请求', (flowName, variant) => {
  const slug = 'gpt-6.2-' + variant
  const model = 'codex-' + slug
  const output =
    flowName === 'compact_text'
      ? [{ type: 'compaction', encrypted_content: 'summary' }]
      : [{ type: 'message', content: [{ type: 'output_text', text: '你好' }] }]
  const files = codex.flows[flowName].steps[0].script_files.map((name) => 'codex/scripts/' + name)
  const run = scripts(files, {
    call: () => ({ tokens: { access_token: 'test' } }),
    checkpoint: () => ({}),
    tryCall: () => ({
      success: true,
      data: {
        ok: true,
        body: {
          models: [
            {
              slug,
              default_reasoning_level: 'low',
              supported_reasoning_levels: [{ effort: 'low' }],
            },
          ],
        },
      },
    }),
    stream: (_action, request, consume) => {
      expect(request.body.model).toBe(slug)
      expect(request.body.reasoning.effort).toBe('low')
      if (flowName === 'compact_text')
        expect(request.body.input.at(-1).type).toBe('compaction_trigger')
      consume({ type: 'response.completed', response: { id: 'resp_test', model: slug, output } })
      return { ok: true, status: 200 }
    },
  })
  const result = run("main({profile_dir:'.',request:{model,input:'你好'}})", { model })
  expect(result.success).toBe(true)
  expect(result.response.output).toEqual(output)
  if (flowName === 'generate_text') expect(result.response.model).toBe(model)
})
