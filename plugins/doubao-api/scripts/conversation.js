let doubaoUuidNode

function doubaoRandomBytes(length) {
  const value = host.call('crypto.random_base64url', { bytes: length }).value
  if (typeof value !== 'string') throw new Error('invalid_response: 随机字节回执无效')
  const text = value.replace(/-/g, '+').replace(/_/g, '/')
  return host.call('crypto.base64_decode_latin1', {
    text: text.padEnd(Math.ceil(text.length / 4) * 4, '='),
  }).value
}

function doubaoLocalConversationId() {
  const bytes = doubaoRandomBytes(16)
  if (typeof bytes !== 'string' || bytes.length !== 16)
    throw new Error('invalid_response: 会话随机字节无效')
  return (
    'local_' +
    bytes
      .split('')
      .map((char) => char.charCodeAt(0))
      .join('')
      .slice(0, 16)
  )
}

function doubaoMessageId() {
  if (!doubaoUuidNode) {
    const bytes = doubaoRandomBytes(6)
    if (typeof bytes !== 'string' || bytes.length !== 6)
      throw new Error('invalid_response: UUID 节点随机字节无效')
    const node = String.fromCharCode(bytes.charCodeAt(0) | 1) + bytes.slice(1)
    doubaoUuidNode = host.call('crypto.base64_encode_latin1', { text: node }).value
  }
  return host.call('crypto.uuid_v1', { node_base64: doubaoUuidNode }).value
}

function doubaoPayload(prompt, kind, options = {}) {
  const id = apiTextUuid()
  const media = kind === 'image' || kind === 'video'
  if (media && (typeof options.model !== 'string' || !options.model.trim()))
    throw new Error('invalid_input: 必须指定具体的豆包生成模型')
  const thinking = options.reasoning && options.reasoning !== 'none'
  const init = { need_ack_conversation: true }
  const collectionId = options.references?.length ? apiTextUuid() : ''
  const displayed =
    kind === 'video'
      ? '生成视频：' +
        prompt +
        (options.ratio && options.ratio !== 'auto' ? '，' + options.ratio : '') +
        '，' +
        (options.duration || 5) +
        's'
      : kind === 'image'
        ? '生成图片：' +
          prompt +
          (options.ratio && options.ratio !== 'auto' ? '，' + options.ratio : '')
        : prompt
  const payload = {
    client_meta: {
      local_conversation_id: options.conversationId || doubaoLocalConversationId(),
      conversation_id: '',
      bot_id: '7338286299411103781',
      last_section_id: '',
      last_message_index: null,
      local_permissions: [
        'ACCESS_COARSE_LOCATION',
        'ACCESS_FINE_LOCATION',
        'ACCESS_BACKGROUND_LOCATION',
      ].map((permission_name) => ({ permission_name, status: 3 })),
    },
    messages: [
      {
        local_message_id: doubaoMessageId(),
        message_status: 0,
        content_block: [
          {
            block_type: 10000,
            block_id: apiTextUuid(),
            parent_id: '',
            meta_info: [],
            append_fields: [],
            content: {
              text_block: { text: displayed, icon_url: '', icon_url_dark: '', summary: '' },
              pc_event_block: '',
            },
          },
        ],
      },
    ],
    option: {
      send_message_scene: '',
      create_time_ms: Date.now(),
      collect_id: collectionId,
      is_audio: false,
      answer_with_suggest: false,
      tts_switch: false,
      need_deep_think: thinking ? 1 : 0,
      click_clear_context: false,
      from_suggest: false,
      is_regen: false,
      is_replace: false,
      is_from_click_option: false,
      is_from_click_softlink: false,
      disable_sse_cache: false,
      select_text_action: '',
      is_select_text: false,
      resend_for_regen: false,
      scene_type: 0,
      unique_key: id,
      start_seq: 0,
      need_create_conversation: true,
      conversation_init_option: init,
      regen_query_id: [],
      edit_query_id: [],
      regen_instruction: '',
      no_replace_for_regen: false,
      message_from: 0,
      shared_app_name: '',
      shared_app_id: '',
      sse_recv_event_options: { support_chunk_delta: true },
      support_lazy_fetch_stream: true,
      is_ai_playground: false,
      is_old_user: true,
      recovery_option: {
        is_recovery: false,
        req_create_time_sec: Math.floor(Date.now() / 1000),
        append_sse_event_scene: 0,
      },
      message_storage_type: 0,
      related_deleted_message_ids: {},
      connector_info_list: [],
      model_config: {
        model_item_key: media ? '' : '0',
        model_extra_params: {},
        ...(!media ? { reasoning_effort: thinking ? 4 : 3 } : {}),
      },
      aggregate_params: {
        mention_skill_list: '[]',
        mention_plugin_list: '[]',
        mention_ext: '[{}]',
        conversation_mode: '1',
        mode_id: media ? '' : '1',
        model_item_key: media ? '' : '0',
        agent_mode: media ? '' : '2',
        reasoning_effort: media ? '' : thinking ? '4' : '3',
        provider_id: '',
      },
      conversation_mode: 1,
      ...(!media
        ? {
            agent_mode: 2,
            conversation_init_ext: {
              model_item_key: '0',
              reasoning_effort: thinking ? '4' : '3',
              mode_id: '1',
            },
          }
        : {}),
    },
    user_context: [],
    ext: {
      ...(media
        ? { answer_with_suggest: '0' }
        : { agent_mode: '2', use_deep_think: thinking ? '1' : '0' }),
      sub_conv_firstmet_type: '1',
      collection_id: collectionId,
      is_finish: '1',
      conversation_init_option: JSON.stringify(init),
      commerce_credit_config_enable: '0',
    },
  }
  if (options.references?.length)
    payload.messages.unshift({
      local_message_id: options.reference_message_id,
      message_status: 0,
      content_block: options.references,
    })
  if (kind === 'image')
    payload.chat_ability = {
      ability_type: 3,
      ability_param: JSON.stringify({
        ability_type: 1,
        ability_param: {
          model: options.model,
          ratio: options.ratio || 'auto',
          input_box_content: { user_input_content: prompt, reply_message_format: '生成图片：%s' },
        },
      }),
    }
  if (kind === 'video')
    payload.chat_ability = {
      ability_type: 17,
      ability_param: JSON.stringify({
        ratio: options.ratio || 'auto',
        model: options.model,
        duration: options.duration || 5,
        input_box_content: { user_input_content: prompt, reply_message_format: '生成视频：%s' },
      }),
    }
  return payload
}

function doubaoStreamState() {
  return {
    text: '',
    blocks: {},
    lastTextBlock: '',
    finished: false,
    conversationId: '',
    messageIds: [],
    legacyMessageId: '',
    legacyText: '',
  }
}

function doubaoLegacyText(content) {
  if (Number(content?.content_type) !== 1 || typeof content.content !== 'string') return ''
  let value
  try {
    value = JSON.parse(content.content)
  } catch (_) {
    throw new Error('invalid_response: 豆包正文不是有效 JSON')
  }
  return typeof value?.text === 'string' ? value.text : ''
}

function doubaoConsumeLegacyText(state, text, append, onText) {
  const previous = state.legacyText
  if (!append && !text.startsWith(previous))
    throw new Error('generation_incomplete: 豆包已输出文本被改写，请恢复原会话')
  const delta = append ? text : text.slice(previous.length)
  state.legacyText = append ? previous + text : text
  state.text += delta
  if (delta && onText) onText(delta)
}

function doubaoIsAnswerBlock(block, blocks) {
  if (Number(block?.block_type) !== 10000) return false
  const visited = new Set()
  while (block.parent_id) {
    if (visited.has(block.parent_id)) return false
    visited.add(block.parent_id)
    block = blocks[block.parent_id]
    if (!block || Number(block.block_type) === 10040 || block.content?.thinking_block) return false
  }
  return true
}

function doubaoConsume(state, event, onText, session) {
  const data = apiSessionEventData(event)
  if (event.event === 'STREAM_ERROR') {
    if (!state.conversationId && Number(data.error_code) === 710022004)
      host.checkpoint({ ...host.checkpoint(), phase: 'ready' })
    doubaoCheckError({ ...data, code: data.error_code, message: data.error_msg }, session)
    throw new Error('platform_rejected: 豆包生成被拒绝')
  }
  doubaoCheckError(data, session)
  if (event.event === 'SSE_ERROR') throw new Error('platform_rejected: 豆包拒绝生成')
  if (data.ack_client_meta?.conversation_id) {
    state.conversationId = data.ack_client_meta.conversation_id
    host.checkpoint({
      ...host.checkpoint(),
      phase: 'accepted',
      conversation_id: state.conversationId,
    })
  }
  if (event.event === 'CHUNK_DELTA' && typeof data.text === 'string') {
    const block = state.blocks[state.lastTextBlock]
    if (!block) throw new Error('invalid_response: 豆包文本增量缺少所属区块')
    block.content.text_block.text = (block.content.text_block.text || '') + data.text
    if (doubaoIsAnswerBlock(block, state.blocks)) {
      state.text += data.text
      if (onText) onText(data.text)
    }
  }
  const acceptBlocks = (blocks) => {
    for (const block of blocks || []) {
      const text = block.content?.text_block?.text
      const previous = state.blocks[block.block_id]
      const append = block.patch_type === 1
      const oldText = previous?.content?.text_block?.text || ''
      if (block.content && Object.keys(block.content).length) {
        state.blocks[block.block_id] = {
          ...previous,
          ...block,
          content: { ...previous?.content, ...block.content },
        }
        if (block.content.text_block)
          state.blocks[block.block_id].content.text_block.text = append
            ? oldText + (text || '')
            : (text ?? oldText)
      }
      if (block.content?.text_block) state.lastTextBlock = block.block_id
      if (
        typeof text === 'string' &&
        doubaoIsAnswerBlock(state.blocks[block.block_id], state.blocks)
      ) {
        if (!append && !text.startsWith(oldText))
          throw new Error('generation_incomplete: 豆包已输出文本被改写，请恢复原会话')
        const delta = append ? text : text.slice(oldText.length)
        state.text += delta
        if (onText && delta) onText(delta)
      }
    }
  }
  if (event.event === 'STREAM_MSG_NOTIFY') {
    doubaoCheckCommerce(data.content)
    if (data.meta?.message_id) state.messageIds.push(data.meta.message_id)
    if (Number(data.meta?.user_type) === 2 && Number(data.content?.content_type) === 1) {
      state.legacyMessageId = data.meta.message_id
      state.legacyText = ''
      doubaoConsumeLegacyText(state, doubaoLegacyText(data.content), false, onText)
    }
    acceptBlocks(data.content?.content_block)
  }
  if (event.event === 'STREAM_CHUNK') {
    for (const patch of data.patch_op || []) {
      if (patch.patch_object === 1) acceptBlocks(patch.patch_value?.content_block)
      if (
        patch.patch_object === 102 &&
        data.message_id === state.legacyMessageId &&
        [1, 2].includes(patch.patch_type)
      )
        doubaoConsumeLegacyText(
          state,
          doubaoLegacyText({ content_type: 1, content: patch.patch_value?.content }),
          patch.patch_type === 1,
          onText
        )
    }
  }
  if (event.event === 'SSE_REPLY_END' && data.end_type >= 2) state.finished = true
}

function doubaoSubmit(session, payload, onText) {
  const state = doubaoStreamState()
  state.conversationId = payload.client_meta.conversation_id || ''
  if (payload.option.need_create_conversation) {
    doubaoInitializeConversation(session, payload)
    doubaoRememberConversationMode(session, payload.option.need_deep_think === 1)
  }
  host.checkpoint({ ...host.checkpoint(), phase: 'submitting' })
  const request = doubaoSignedRequest(session, '/chat/completion', payload, 'POST', {
    accept: 'text/event-stream',
    'agw-js-conv': 'str, str',
    'last-event-id': 'undefined',
    'x-flow-trace': doubaoFlowTrace(),
  })
  let receivedHeaders = false
  const response = apiSessionRequireOk(
    host.stream(
      'http.sse_request',
      {
        ...request,
        timeout_ms: 10800000,
        include_sse_metadata: true,
        include_http_metadata: true,
        collect_body: true,
        idle_interval_ms: 1000,
      },
      (event) => {
        if (event.type === 'http_response') {
          receivedHeaders = true
          doubaoRefreshSecurity(session, event, '/chat/completion')
          return
        }
        session.securityRuntime?.tick()
        doubaoHeartbeatTick(session)
        if (event.type === 'http_idle') return null
        doubaoConsume(state, event, onText, session)
      }
    ),
    '豆包生成失败'
  )
  if (!receivedHeaders) doubaoRefreshSecurity(session, response, '/chat/completion')
  else {
    session.securityRuntime.finish(response)
    doubaoPersistSecurityStorage(session)
  }
  if (!state.finished) throw new Error('generation_incomplete: 豆包事件流提前结束')
  if (!state.conversationId) throw new Error('PROVIDER_SUBMISSION_UNKNOWN: 豆包未返回会话编号')
  doubaoSyncConversation(session, state.conversationId, payload.option.need_deep_think === 1)
  return state
}

function doubaoConfirmationPayload(conversation, kind) {
  if (
    !/^\d+$/.test(String(conversation?.conversation_id || '')) ||
    !/^\d+$/.test(String(conversation.last_section_id || '')) ||
    !Number.isSafeInteger(Number(conversation.latest_index))
  )
    throw new Error('invalid_response: 豆包原会话缺少继续生成所需信息')
  const payload = doubaoPayload(
    '确认，按上述参数直接生成' + (kind === 'image' ? '图片' : '视频') + '。',
    'text',
    { conversationId: String(conversation.conversation_id) }
  )
  delete payload.client_meta.local_conversation_id
  Object.assign(payload.client_meta, {
    conversation_id: String(conversation.conversation_id),
    last_section_id: String(conversation.last_section_id),
    last_message_index: Number(conversation.latest_index),
  })
  const task = {
    runtime_type: 0,
    agent_task_param: {},
    agent_task_param_change: {
      runtime_changed: false,
      device_changed: false,
      sandbox_auth_type_changed: false,
    },
    project_id: '',
    need_modify_conversation: false,
  }
  delete payload.option.conversation_init_option
  delete payload.option.conversation_init_ext
  delete payload.option.conversation_mode
  payload.option.need_create_conversation = false
  payload.option.general_task_param = task
  payload.option.aggregate_params.conversation_mode = ''
  delete payload.ext.sub_conv_firstmet_type
  delete payload.ext.conversation_init_option
  payload.ext.general_task_param = JSON.stringify(task)
  return payload
}

function doubaoCreations(state) {
  return Object.values(state.blocks).flatMap(
    (block) => block.content?.creation_block?.creations || []
  )
}
