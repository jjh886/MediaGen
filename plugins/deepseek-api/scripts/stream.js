function deepseekStreamState() {
  return {
    path: '',
    operation: 'APPEND',
    fragments: [],
    finished: false,
    status: '',
    responseId: null,
    output: '',
  }
}

function deepseekEmitFragments(state, writer) {
  const text = state.fragments
    .filter((fragment) => fragment.type === 'RESPONSE')
    .map((fragment) => fragment.content || '')
    .join('')
  if (!text.startsWith(state.output)) throw new Error('invalid_response: DeepSeek 已输出文本被改写')
  apiTextFeedResponseText(writer, text.slice(state.output.length))
  state.output = text
}

function deepseekSnapshot(state, writer, message) {
  if (!Array.isArray(message?.fragments))
    throw new Error('invalid_response: DeepSeek 完整消息缺少内容片段')
  state.fragments = message.fragments.map((fragment) => ({ ...fragment }))
  state.status = message.status || ''
  state.finished = state.status === 'FINISHED'
  if (message.message_id != null) state.responseId = message.message_id
  if (['ERROR', 'FAILED', 'STOPPED'].includes(state.status))
    throw new Error('generation_incomplete: DeepSeek 原消息已终止')
  deepseekEmitFragments(state, writer)
}

function deepseekConsume(state, writer, event, checkpoint) {
  if (event?.data?.biz_code !== undefined) {
    const code = Number(event.data.biz_code)
    if (Number(event.code) !== 0 || ![0, 22].includes(code))
      throw new Error('platform_rejected: DeepSeek 恢复原消息失败')
    const data = event.data.biz_data
    const message = [
      data?.message,
      data?.response?.message,
      data?.response,
      data?.chat_message,
      data,
    ].find((item) => Array.isArray(item?.fragments))
    deepseekSnapshot(state, writer, message)
    return
  }
  const data = apiSessionEventData(event)
  if (event.event === 'ready') {
    if (!Number.isSafeInteger(data.response_message_id) || data.response_message_id <= 0)
      throw new Error('invalid_response: DeepSeek 消息编号无效')
    state.responseId = data.response_message_id
    checkpoint(state.responseId)
    return
  }
  if (event.event === 'error' || data.error) throw new Error('platform_rejected: DeepSeek 生成失败')
  if (!Object.prototype.hasOwnProperty.call(data, 'v')) return
  deepseekPatch(state, writer, data, '')
}

function deepseekPatch(state, writer, patch, prefix) {
  if (patch.p !== undefined) state.path = prefix + patch.p
  if (patch.o !== undefined) state.operation = patch.o
  const path = state.path
  const operation = state.operation
  const value = patch.v
  if (operation === 'BATCH' && Array.isArray(value)) {
    for (const child of value) deepseekPatch(state, writer, child, path + '/')
    return
  }
  if (value?.response) {
    deepseekSnapshot(state, writer, value.response)
  } else if (path === 'response/fragments' && Array.isArray(value)) {
    if (operation === 'SET') state.fragments = []
    for (const fragment of value) state.fragments.push({ ...fragment })
    deepseekEmitFragments(state, writer)
  } else if (/^response\/fragments\/(-1|\d+)\/content$/.test(path) && typeof value === 'string') {
    const index = Number(path.split('/')[2])
    const fragment = state.fragments[index < 0 ? state.fragments.length - 1 : index]
    if (!fragment) throw new Error('invalid_response: DeepSeek 内容片段缺失')
    fragment.content = operation === 'SET' ? value : (fragment.content || '') + value
    deepseekEmitFragments(state, writer)
  } else if (path === 'response/status') {
    state.status = value
    state.finished = value === 'FINISHED'
    if (['ERROR', 'FAILED', 'STOPPED'].includes(value))
      throw new Error('generation_incomplete: DeepSeek 原消息已终止')
  }
}
