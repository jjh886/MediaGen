function doubaoFlowTrace() {
  const randomId = () => Math.floor(Math.random() * Number.MAX_SAFE_INTEGER + 1)
  const hex = (value) => value.toString(16).padStart(16, '0')
  let span = randomId() + 1
  while (span >= Number.MAX_SAFE_INTEGER) span = randomId() + 1
  return '04-' + hex(randomId()) + hex(randomId()) + '-' + hex(span) + '-01'
}

function doubaoImRequest(session, path, cmd, uplink, timeoutMs = 60000) {
  const response = doubaoRequest(
    session,
    path,
    {
      cmd,
      uplink_body: uplink,
      sequence_id: apiTextUuid(),
      channel: 2,
      version: '1',
    },
    'POST',
    timeoutMs
  )
  if (Number(response.status_code) !== 0) throw new Error('temporary_unavailable: 豆包会话同步失败')
  return response
}

function doubaoRememberConversationMode(session, thinking) {
  doubaoRequest(session, '/alice/slot/action_bar_v3/thinkmode_memory_report', {
    memory_scene: 'new_chat',
    memory_item: {
      mode_id: '1',
      model_item: { item_id: '0', model_item_key: '0' },
      model_reasoning_effort: thinking ? 4 : 3,
      agent_mode: 2,
    },
    behavior_source: 2,
    conversation_source: 2,
  })
}

function doubaoConversationActions(session, conversationId) {
  if (session.actionBar?.conversationId === conversationId) return session.actionBar.items
  const body = {
    language_code: session.auth.protocol_query?.language || 'zh',
    bot_id: '7338286299411103781',
    conversation_id: conversationId,
  }
  doubaoRequest(session, '/alice/office/skills/list_user_and_featured', { runtime_type: 1 })
  doubaoRequest(session, '/alice/bot/action_bar_v3/active_switch_banner', body)
  const brief = doubaoRequest(session, '/alice/slot/action_bar_v3/brief_list', body)
  if (!Array.isArray(brief.entry_list)) throw new Error('invalid_response: 豆包未返回会话能力列表')
  const items = brief.entry_list
  const ids = items.map((item) => item.item_id)
  if (ids.some((id) => typeof id !== 'string' || !/^\d+$/.test(id)))
    throw new Error('invalid_response: 豆包会话能力编号无效')
  const conf = ids.length
    ? doubaoRequest(session, '/alice/slot/action_bar_v3/get_item_conf', { ...body, item_ids: ids })
    : {}
  Object.defineProperty(session, 'actionBar', {
    value: { conversationId, items, conf },
    configurable: true,
    writable: true,
    enumerable: false,
  })
  return items
}

function doubaoInitializeConversation(session, payload) {
  const items = doubaoConversationActions(session, payload.client_meta.local_conversation_id)
  const ability = payload.chat_ability?.ability_type
  const instruction = ability === 3 ? 4 : ability === 17 ? 18 : undefined
  if (instruction !== undefined) {
    const item = items.find((entry) => Number(entry.instruction_type) === instruction)
    if (!item) throw new Error('temporary_unavailable: 当前账号未开放所选生成能力')
    doubaoRequest(session, '/alice/slot/action_bar_v3/update_order', {
      bot_id: payload.client_meta.bot_id,
      last_used_item_id: item.item_id,
    })
  }
}

function doubaoSyncConversation(session, conversationId, thinking) {
  doubaoImRequest(session, '/im/conversation/batch_get', 1111, {
    batch_get_conv_info_uplink_body: {
      conversation_id: [conversationId],
      option: { recent_message_count_per_conv: 1 },
      ext: {},
    },
  })
  doubaoImRequest(session, '/im/conversation/modify', 1114, {
    modify_conversation_uplink_body: {
      conversation_id: conversationId,
      conversation_type: 3,
      bot_conversation_type: 3,
      mode_id: '1',
      model_item_key: '0',
      reasoning_effort: thinking ? '4' : '3',
    },
  })
  doubaoImRequest(session, '/im/conversation/info', 1110, {
    get_conv_info_uplink_body: {
      conversation_id: conversationId,
      ext: { cold_start: 'true' },
      bot_id: '',
      conversation_type: 3,
      option: { need_bot_info: true },
    },
  })
  doubaoConversationActions(session, conversationId)
}

function doubaoMarkConversationRead(session, conversationId) {
  doubaoImRequest(session, '/im/message/mark_conv_read', 2100, {
    mark_conv_read_uplink_body: { conversation_id: conversationId, conversation_type: 3 },
  })
}
