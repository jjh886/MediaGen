import { readFileSync } from 'node:fs'
import vm from 'node:vm'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { describe, expect, it, vi } from 'vitest'

function fixture(saved = {}) {
  let checkpoint = saved
  const context = vm.createContext({
    host: {
      checkpoint(value) {
        if (value !== undefined) checkpoint = JSON.parse(JSON.stringify(value))
        return checkpoint
      },
    },
  })
  vm.runInContext(
    readFileSync(
      path.resolve(
        path.dirname(fileURLToPath(import.meta.url)),
        '../../../plugins/chatgpt/scripts/conversation.js'
      ),
      'utf8'
    ),
    context
  )
  Object.assign(context, {
    chatgptApiFingerprint: () => ({}),
    apiTextUuid: () => 'id',
    chatgptApiCompactObject: (value) => value,
    chatgptApiSentinelHeaders: () => ({}),
    chatgptApiEchoLogs: () => '',
    chatgptApiRequireOk: (value) => value,
    chatgptApiWaitForImage: vi.fn(() => ({ url: 'https://test.local/image' })),
  })
  return { context, checkpoint: () => checkpoint }
}
describe('图片实际提示词与恢复检查点', () => {
  it('缺少当前宿主检查点能力时立即失败，不提交无法恢复的生成任务', () => {
    const f = fixture()
    delete f.context.host.checkpoint
    f.context.chatgptApiRequest = vi.fn()
    expect(() => f.context.chatgptApiGenerateConversation({}, '测试', '1:1', [], {})).toThrow(
      'checkpoint'
    )
    expect(f.context.chatgptApiRequest).not.toHaveBeenCalled()
  })
  it('构造请求没有检查点副作用，发送时保留原会话状态', () => {
    const f = fixture({
      phase: 'submitting',
      images: [{ file_id: 'first' }],
      references: [{ file_id: 'ref' }],
    })
    const payload = f.context.chatgptApiConversationPayload(
      {},
      'auto',
      '少女推开门',
      '16:9',
      [],
      ''
    )
    expect(f.checkpoint().submitted_prompt).toBeUndefined()
    f.context.chatgptApiRequest = vi.fn((_auth, _method, _path, options) => {
      expect(f.checkpoint().phase).toBe('submitting')
      expect(f.checkpoint().images[0].file_id).toBe('first')
      expect(f.checkpoint().references[0].file_id).toBe('ref')
      expect(f.checkpoint().submitted_prompt).toBe(options.body.messages[0].content.parts[0])
      return { body: 'ok' }
    })
    f.context.chatgptApiSendConversation({}, '少女推开门', '16:9', {
      model: 'auto',
      references: [],
    })
    expect(f.checkpoint().submitted_prompt).toBe(payload.messages[0].content.parts[0])
  })
  it('恢复时回读原始实发提示词，不拿恢复入参重建证据', () => {
    const f = fixture({
      phase: 'accepted',
      conversation_id: 'original',
      submitted_prompt: '原请求实际正文',
      model: 'auto',
    })
    const result = f.context.chatgptApiGenerateConversation({}, '另一份输入', '1:1', [], {})
    expect(result.submitted_prompt).toBe('原请求实际正文')
    expect(f.context.chatgptApiWaitForImage).toHaveBeenCalledWith({}, 'original')
  })
})
