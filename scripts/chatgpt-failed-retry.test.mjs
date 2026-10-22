import assert from 'node:assert/strict'
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import vm from 'node:vm'
import { test } from 'node:test'
const scripts = resolve(dirname(fileURLToPath(import.meta.url)), '../plugins/chatgpt/scripts')

for (const legacy of [
  { phase: 'failed' },
  { phase: 'accepted', poll_rate_limited: true },
  { phase: 'accepted', poll_next_at_ms: 1 },
]) {
  test(`重启后仅重置明确失败的新会话，限流中的已受理会话保留：${JSON.stringify(legacy)}`, () => {
    const dir = mkdtempSync(join(tmpdir(), 'failed-conversation-'))
    const path = join(dir, 'checkpoint.json')
    const initial = {
      ...legacy,
      conversation_id: 'failed',
      images: [{ url: 'data:image/png;base64,saved' }],
      attachments: [{ file_id: 'video', mime_type: 'video/mp4' }],
    }
    writeFileSync(path, JSON.stringify(initial))
    const host = {
      checkpoint(value) {
        if (value) writeFileSync(path, JSON.stringify(value))
        return JSON.parse(readFileSync(path, 'utf8'))
      },
    }
    const reopen = () => {
      const context = vm.createContext({ host })
      for (const file of ['conversation.js', 'poll_retry.js'])
        vm.runInContext(readFileSync(join(scripts, file), 'utf8'), context)
      return context
    }
    try {
      const first = reopen().chatgptApiPrepareConversationAttempt()
      assert.equal(first.phase, legacy.phase === 'failed' ? 'ready' : 'accepted')
      assert.equal(first.conversation_id, legacy.phase === 'failed' ? undefined : 'failed')
      const restarted = reopen().chatgptApiPrepareConversationAttempt()
      assert.equal(restarted.phase, legacy.phase === 'failed' ? 'ready' : 'accepted')
      assert.equal(restarted.attachments[0].file_id, 'video')
      assert.equal(restarted.images.length, 1)
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })
}

test('已受理会话限流保留原编号，新会话的拒绝允许宿主换号，完成结果不重投', () => {
  let saved = {
    phase: 'accepted',
    conversation_id: 'original',
    images: [{ url: 'data:image/png;base64,saved' }],
  }
  let now = 1000
  const context = vm.createContext({
    Date: class extends Date {
      static now() {
        return now
      }
    },
    host: {
      checkpoint(value) {
        if (value) saved = JSON.parse(JSON.stringify(value))
        return saved
      },
    },
  })
  for (const file of ['conversation.js', 'poll_retry.js'])
    vm.runInContext(readFileSync(join(scripts, file), 'utf8'), context)
  context.chatgptApiRequireRetryableRead({ status: 429, coalesced: true })
  assert.equal(saved.conversation_id, 'original')
  assert.doesNotThrow(() =>
    context.chatgptApiRequireRetryableRead({ status: 429, headers: { 'retry-after': '120' } })
  )
  assert.equal(saved.conversation_id, 'original')
  assert.equal(saved.phase, 'accepted')
  saved = { phase: 'submitting', images: saved.images }
  assert.throws(
    () => context.chatgptApiRequireRetryableRead({ status: 429 }),
    /rate_limit_exceeded/
  )
  assert.equal(saved.conversation_id, undefined)
  assert.equal(saved.phase, 'ready')
  assert.equal(saved.attachments, undefined)
  assert.equal(context.chatgptApiPrepareConversationAttempt().phase, 'ready')
  assert.equal(saved.images.length, 1)
  saved = {
    phase: 'completed',
    poll_rate_limited: true,
    result: { response: { output_text: '已完成' } },
  }
  assert.equal(context.chatgptApiPrepareConversationAttempt().result.response.output_text, '已完成')
})

test('固定视频反推的真实429结束本轮，附件保留并在Retry-After后重试；本机排队不重投', () => {
  let now = 1000
  let saved = {
    phase: 'accepted',
    conversation_id: 'video-original',
    attachments: [{ file_id: 'full-video', mime_type: 'video/mp4' }],
  }
  const context = vm.createContext({
    Date: class extends Date {
      static now() {
        return now
      }
    },
    host: {
      checkpoint(value) {
        if (value !== undefined) saved = JSON.parse(JSON.stringify(value))
        return saved
      },
    },
  })
  for (const file of ['conversation.js', 'poll_retry.js'])
    vm.runInContext(readFileSync(join(scripts, file), 'utf8'), context)
  vm.runInContext('chatgptApiFixedVideoAnalysis = true', context)
  context.chatgptApiRequireRetryableRead({ status: 429, coalesced: true })
  assert.equal(saved.conversation_id, 'video-original')
  assert.throws(
    () =>
      context.chatgptApiRequireRetryableRead({
        status: 429,
        headers: { 'retry-after': '120' },
      }),
    /upstream_rate_limited/
  )
  assert.equal(saved.phase, 'retry_pending')
  assert.equal(saved.conversation_id, undefined)
  assert.equal(saved.failed_conversation_id, 'video-original')
  assert.equal(saved.retry_not_before_ms, 121000)
  assert.equal(saved.attachments[0].file_id, 'full-video')
  assert.throws(() => context.chatgptApiPrepareConversationAttempt(), /重试等待/)
  now = 121000
  const next = context.chatgptApiPrepareConversationAttempt()
  assert.equal(next.phase, 'ready')
  assert.equal(next.reset_session, true)
  assert.equal(next.attachments[0].file_id, 'full-video')
  saved = { phase: 'completed', result: { response: { output_text: '完整反推稿' } } }
  context.chatgptApiRequireRetryableRead({ status: 429 })
  assert.equal(saved.phase, 'completed')
})

test('图片提交结果不明时 ChatGPT 与 Codex 都不重投，已收到图片仍可恢复', () => {
  let saved = { phase: 'submitting' }
  const ctx = vm.createContext({
    host: {
      checkpoint(value) {
        if (value) saved = value
        return saved
      },
    },
  })
  for (const file of [
    join(scripts, 'conversation.js'),
    join(scripts, '../../codex/scripts/generate_image.js'),
  ])
    vm.runInContext(readFileSync(file, 'utf8'), ctx)
  assert.throws(
    () => ctx.chatgptApiGenerateConversation({}, 'draw', '', [], {}, {}),
    /PROVIDER_SUBMISSION_UNKNOWN/
  )
  assert.throws(
    () => ctx.requestImageWithRetry('token', 'account', {}, {}),
    /PROVIDER_SUBMISSION_UNKNOWN/
  )
  saved = { phase: 'image_ready', pending_image: { image: 'saved-image' } }
  assert.equal(ctx.requestImageWithRetry('token', 'account', {}, {}).image, 'saved-image')
})

test('Codex 图片明确 4xx 拒绝可重试，408 和 5xx 保持提交未确认', () => {
  for (const status of [400, 401, 404, 408, 429, 500]) {
    let saved = { phase: 'ready' }
    const ctx = vm.createContext({
      host: {
        checkpoint(value) {
          if (value) saved = value
          return saved
        },
        stream() {
          assert.equal(saved.phase, 'submitting')
          return { ok: false, status, body: { error: { message: 'upstream failure' } } }
        },
      },
    })
    for (const file of [
      'auth.js',
      'retry.js',
      'image_response.js',
      'direct_images.js',
      'generate_image.js',
    ])
      vm.runInContext(readFileSync(join(scripts, '../../codex/scripts', file), 'utf8'), ctx)
    ctx.requestImageStream({}, 'https://example.test/images', {})
    assert.equal(saved.phase, status < 500 && status !== 408 ? 'ready' : 'submitting')
  }
})

test('ChatGPT 图片逐请求思考深度覆盖默认值，账号不支持时拒绝而不降档', () => {
  const ctx = vm.createContext({})
  for (const file of ['conversation.js', 'generate_image.js'])
    vm.runInContext(readFileSync(join(scripts, file), 'utf8'), ctx)
  const request = {
    model_id: 'chatgpt-image-gpt-6-pro',
    reasoning_effort: 'standard',
    platform_config: { thinking_effort: 'extended' },
    model_capabilities: { reasoning: { efforts: ['standard'], default_effort: 'standard' } },
  }
  const bootstrap = {
    '/backend-api/models': {
      models: [
        {
          slug: 'gpt-6-pro',
          thinking_efforts: ['standard'],
          default_thinking_effort: 'standard',
        },
      ],
    },
  }
  const result = ctx.chatgptApiGenerationConfig(request, bootstrap, {})
  assert.equal(result.model, 'gpt-6-pro')
  assert.equal(result.thinking_effort, 'standard')
  assert.throws(
    () => ctx.chatgptApiGenerationConfig({ ...request, reasoning_effort: 'high' }, bootstrap, {}),
    /不支持思考深度 high/
  )
})
