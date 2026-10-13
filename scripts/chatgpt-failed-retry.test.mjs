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
  test(`历史失败记录重启后必须准备新会话：${JSON.stringify(legacy)}`, () => {
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
      assert.equal(first.phase, 'ready')
      assert.equal(first.conversation_id, undefined)
      const restarted = reopen().chatgptApiPrepareConversationAttempt()
      assert.equal(restarted.phase, 'ready')
      assert.equal(restarted.attachments[0].file_id, 'video')
      assert.equal(restarted.images.length, 1)
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })
}

test('原任务遇到真实限流会持久化失败和等待时间，本机排队与已完成结果不重投', () => {
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
  assert.throws(
    () =>
      context.chatgptApiRequireRetryableRead({ status: 429, headers: { 'retry-after': '120' } }),
    /原任务已失败/
  )
  assert.equal(saved.failed_conversation_id, 'original')
  assert.equal(saved.conversation_id, undefined)
  assert.equal(saved.retry_not_before_ms, 121000)
  assert.equal(saved.attachments, undefined)
  assert.throws(() => context.chatgptApiPrepareConversationAttempt(), /重试等待/)
  now = 121001
  assert.equal(context.chatgptApiPrepareConversationAttempt().phase, 'ready')
  assert.equal(saved.images.length, 1)
  saved = {
    phase: 'completed',
    poll_rate_limited: true,
    result: { response: { output_text: '已完成' } },
  }
  assert.equal(context.chatgptApiPrepareConversationAttempt().result.response.output_text, '已完成')
})
