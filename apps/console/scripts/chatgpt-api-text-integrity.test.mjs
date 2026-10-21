import { readFileSync } from 'node:fs'
import vm from 'node:vm'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { beforeEach, expect, it } from 'vitest'

let context
let events
beforeEach(() => {
  events = []
  context = vm.createContext({ host: { emit: (event) => events.push(event) } })
  for (const file of [
    'auth',
    'poll_retry',
    'conversation_results',
    '../../_shared/api_text_attachments',
    '../../_shared/api_text_responses',
    '../../_shared/api_text_media',
    'text_updates',
    'text_media',
    'text_conversation',
  ]) {
    vm.runInContext(
      readFileSync(
        path.resolve(
          path.dirname(fileURLToPath(import.meta.url)),
          `../../../plugins/chatgpt/scripts/${file}.js`
        ),
        'utf8'
      ),
      context
    )
  }
  vm.runInContext(
    `var state = chatgptApiCreateTextStreamState(); var writer = apiTextCreateResponseWriter({tools: []});`,
    context
  )
})

function run(code) {
  return vm.runInContext(code, context)
}
function message(id, channel, text) {
  context.event = {
    message: {
      id,
      channel,
      author: { role: 'assistant' },
      content: { parts: [text] },
      end_turn: true,
    },
  }
  run('chatgptApiConsumeTextStreamEvent(state, writer, event)')
}
function complete(text) {
  context.final = text
  return run(
    'chatgptApiAppendMissingFinalText(state, writer, final); apiTextCompleteResponse(writer)'
  )
}

it('逐个保留重复的引号、右括号和相同字词', () => {
  for (const chunks of [
    ['{"value":"', '"', '}'],
    ['{"nested":{', '}', '}'],
    ['哈', '哈'],
  ]) {
    context.chunks = chunks
    expect(run('chunks.reduce(chatgptApiAppendText, "")')).toBe(chunks.join(''))
  }
})
it('commentary 与 analysis 不混入最终 JSON', () => {
  message('a', 'commentary', '我会严格按注册表复制稳定 ID。')
  message('b', 'analysis', '分析')
  message('c', 'final', '{"candidates":[]}')
  expect(complete('{"candidates":[]}').output_text).toBe('{"candidates":[]}')
  expect(
    events
      .filter((e) => e.type === 'response.output_text.delta')
      .map((e) => e.delta)
      .join('')
  ).toBe('{"candidates":[]}')
})
it('最终快照修正已流出的不完整文本，不能追加两份 JSON', () => {
  message('a', 'final', '{"value":"wrong')
  message('a', 'final', '{"value":"correct"}')
  const result = complete('{"value":"correct"}')
  expect(result.output_text).toBe('{"value":"correct"}')
  expect(result.output[0].content[0].text).toBe(result.output_text)
})
it('新消息清理旧 final_text', () => {
  message('a', 'final', '旧消息')
  context.event = { p: '/message/id', v: 'b' }
  run('chatgptApiConsumeTextStreamEvent(state, writer, event)')
  expect(run('state.final_text')).toBe('')
  expect(complete('新消息').output_text).toBe('新消息')
})
it('快照替换后仍完整保留工具调用', () => {
  run('writer.request.tools = [{name: "lookup"}]')
  message('a', 'final', '旧正文')
  const result = complete('<tool_call>{"name":"lookup","arguments":{"name":"角色"}}</tool_call>')
  expect(result.output).toHaveLength(1)
  expect(result.output[0].type).toBe('function_call')
  expect(JSON.parse(result.output[0].arguments)).toEqual({ name: '角色' })
})

it('同一消息的分析频道切到 final 时不带入分析文本', () => {
  context.input = [
    { choices: [{ delta: { role: 'assistant', channel: 'analysis', content: '分析过程' } }] },
    { choices: [{ delta: { channel: 'final', content: '{"value":"' } }] },
    { choices: [{ delta: { content: '"' } }] },
    { choices: [{ delta: { content: '}' }, finish_reason: 'stop' }] },
  ]
  run('input.forEach(event => chatgptApiConsumeTextStreamEvent(state, writer, event))')
  expect(complete('{"value":""}').output_text).toBe('{"value":""}')
  expect(
    events
      .filter((e) => e.type === 'response.output_text.delta')
      .map((e) => e.delta)
      .join('')
  ).toBe('{"value":""}')
})
it('没有 channel 的标准 choices 增量仍实时输出', () => {
  context.event = { choices: [{ delta: { role: 'assistant', content: '你好' } }] }
  run('chatgptApiConsumeTextStreamEvent(state, writer, event)')
  expect(events.some((e) => e.type === 'response.output_text.delta' && e.delta === '你好')).toBe(
    true
  )
})
