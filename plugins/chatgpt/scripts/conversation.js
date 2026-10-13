// 同一次插件执行内复用提交检查点，网络 handoff 仅接续原会话。
function chatgptApiCheckpoint(value) {
  return host.checkpoint(value) || {}
}

function chatgptApiGenerateConversation(
  auth,
  prompt,
  aspectRatio,
  references,
  generationConfig,
  initialized
) {
  const saved = chatgptApiCheckpoint()
  if (saved.conversation_id) {
    return {
      model: saved.model,
      thinking_effort: saved.thinking_effort,
      references: saved.references || references,
      submitted_prompt: saved.submitted_prompt,
      result: chatgptApiWaitForImage(auth, saved.conversation_id),
    }
  }
  // 没有上游会话编号可供查询时，重新提交这一张；已完成图片由外层检查点复用。
  const init = initialized || chatgptApiInitializeConversation(auth)
  chatgptApiAssertImageAvailable(init)
  const options = chatgptApiConversationOptions(init, generationConfig)
  const prepared = chatgptApiPrepareConversation(auth, prompt, references, options)
  chatgptApiCheckpoint({
    ...saved,
    phase: 'submitting',
    model: options.model,
    thinking_effort: options.thinking_effort,
    references: prepared.references,
  })
  const response = chatgptApiSendConversation(auth, prompt, aspectRatio, prepared)
  const submittedPrompt = chatgptApiCheckpoint().submitted_prompt
  const streamResult = response.events || response.body
  const conversationId = chatgptApiFindConversationId(streamResult)
  if (!conversationId)
    throw new Error('PROVIDER_SUBMISSION_UNKNOWN: ChatGPT 会话响应缺少 conversation_id')
  chatgptApiCheckpoint({
    ...chatgptApiCheckpoint(),
    phase: 'accepted',
    submitted_prompt: submittedPrompt,
    conversation_id: conversationId,
    model: options.model,
    thinking_effort: options.thinking_effort,
    references: prepared.references,
  })
  const streamedImage = chatgptApiResolveImageCandidate(
    auth,
    conversationId,
    chatgptApiGeneratedImageCandidate(streamResult)
  )
  return {
    model: options.model,
    thinking_effort: options.thinking_effort,
    references: prepared.references,
    submitted_prompt: submittedPrompt,
    result: streamedImage || chatgptApiWaitForImage(auth, conversationId),
  }
}

function chatgptApiConversationOptions(init, generationConfig) {
  return {
    model: chatgptApiImageModelSlug(generationConfig?.model),
    thinking_effort: chatgptApiThinkingEffort(generationConfig?.thinking_effort),
  }
}

function chatgptApiImageModelSlug(value) {
  const model = String(value || '').trim()
  if (!model || model === 'gpt-image-2' || model.startsWith('gpt-image')) return 'auto'
  return model
}

function chatgptApiPrepareConversation(auth, prompt, references, options) {
  const turnTraceId = apiTextUuid()
  let uploaded = Array.isArray(references) ? references : []
  const needsUpload = uploaded.some((reference) => reference?.path && !reference.file_id)
  let conduit = chatgptApiPrepareConduit(
    auth,
    options.model,
    turnTraceId,
    uploaded,
    needsUpload ? 'file_picker' : 'composer_editor_state',
    options.thinking_effort,
    prompt
  )
  if (needsUpload) {
    const pending = uploaded.filter((reference) => !reference.file_id)
    const completed = chatgptApiUploadReferences(auth, pending)
    let index = 0
    uploaded = uploaded.map((reference) => (reference.file_id ? reference : completed[index++]))
    // 文件处理完成后，网页会立即用 context_change 重新准备一次会话。
    conduit = chatgptApiPrepareConduit(
      auth,
      options.model,
      turnTraceId,
      uploaded,
      'context_change',
      options.thinking_effort,
      prompt
    )
  }
  return { ...options, references: uploaded, turn_trace_id: turnTraceId, conduit }
}

function chatgptApiSendConversation(auth, prompt, aspectRatio, prepared) {
  const sentinelHeaders = chatgptApiSentinelHeaders(auth)
  const request = chatgptApiConversationPayload(
    auth,
    prepared.model,
    prompt,
    aspectRatio,
    prepared.references,
    prepared.thinking_effort
  )
  // 保存完整检查点，恢复时沿用实际请求中的文本。
  chatgptApiCheckpoint({
    ...chatgptApiCheckpoint(),
    submitted_prompt: request.messages[0].content.parts
      .filter((part) => typeof part === 'string')
      .join('\n'),
  })
  return chatgptApiRequireOk(
    chatgptApiRequest(auth, 'POST', '/backend-api/f/conversation', {
      target_route: '/backend-api/f/conversation',
      headers: chatgptApiCompactObject({
        ...sentinelHeaders,
        accept: 'text/event-stream',
        'content-type': 'application/json',
        'oai-echo-logs': chatgptApiEchoLogs(auth),
        'x-conduit-token': prepared.conduit,
        'x-oai-turn-trace-id': prepared.turn_trace_id,
      }),
      body: request,
      sse: true,
      retry: { max_attempts: 1 },
      on_event: (event) => {
        const conversationId = chatgptApiFindConversationId(event)
        if (conversationId) {
          const saved = chatgptApiCheckpoint()
          if (saved.conversation_id !== conversationId) {
            chatgptApiCheckpoint({ ...saved, phase: 'accepted', conversation_id: conversationId })
          }
        }
      },
      timeout_ms: 3 * 60 * 60 * 1000,
    }),
    '发送 ChatGPT 生图消息'
  )
}

function chatgptApiPrepareConduit(
  auth,
  model,
  turnTraceId,
  references,
  source,
  thinkingEffort,
  prompt
) {
  const prepare = chatgptApiRequireOk(
    chatgptApiRequest(auth, 'POST', '/backend-api/f/conversation/prepare', {
      target_path: '/backend-api/f/conversation/prepare',
      target_route: '/backend-api/f/conversation/prepare',
      headers: {
        'content-type': 'application/json',
        'x-oai-turn-trace-id': turnTraceId,
      },
      body: chatgptApiConversationPreparePayload(
        auth,
        model,
        references,
        source,
        thinkingEffort,
        prompt
      ),
    }),
    '准备 ChatGPT 会话'
  ).body
  if (!prepare || (prepare.status && prepare.status !== 'ok')) {
    throw new Error('ChatGPT 会话准备响应无效')
  }
  // 当前网页允许 conduit_token 为 null；这时发送会话时不带对应请求头。
  return typeof prepare.conduit_token === 'string' ? prepare.conduit_token : ''
}

// 会话初始化是独立请求，单独封装后主流程只保留“初始化、准备、发送、轮询”四个步骤。
function chatgptApiInitializeConversation(auth) {
  const fingerprint = chatgptApiFingerprint(auth)
  return chatgptApiRequireOk(
    chatgptApiRequest(auth, 'POST', '/backend-api/conversation/init', {
      target_path: '/backend-api/conversation/init',
      target_route: '/backend-api/conversation/init',
      headers: { 'content-type': 'application/json' },
      body: {
        requested_default_model: null,
        conversation_id: null,
        timezone_offset_min: Number(fingerprint.timezone_offset_min || 0),
        conversation_origin: null,
        gizmo_id: null,
        system_hints: null,
        timezone: fingerprint.timezone || 'UTC',
      },
    }),
    '初始化 ChatGPT 会话'
  ).body
}

function chatgptApiConversationPreparePayload(
  auth,
  model,
  references,
  source,
  _thinkingEffort,
  prompt
) {
  const fingerprint = chatgptApiFingerprint(auth)
  const attachments = Array.isArray(references) ? references : []
  const contextChange = source === 'context_change'
  const payload = chatgptApiCompactObject({
    action: 'next',
    is_do_not_remember: false,
    parent_message_id: 'client-created-root',
    model,
    client_prepare_state: 'sent',
    client_prepare_dispatch: contextChange ? 'immediate' : 'debounced',
    client_prepare_source: source || 'composer_editor_state',
    timezone_offset_min: Number(fingerprint.timezone_offset_min || 0),
    timezone: fingerprint.timezone || 'UTC',
    system_hints: ['picture_v2'],
    partial_query: {
      id: apiTextUuid(),
      author: { role: 'user' },
      content: { content_type: 'text', parts: [String(prompt || '')] },
    },
    attachment_mime_types: chatgptApiUnique(
      attachments.map((reference) => reference.mime_type).filter(Boolean)
    ),
    local_function_names: ['local.continue_in_work'],
  })
  if (!payload.attachment_mime_types.length) delete payload.attachment_mime_types
  return payload
}

function chatgptApiConversationPayload(
  auth,
  model,
  prompt,
  aspectRatio,
  references,
  thinkingEffort
) {
  const fingerprint = chatgptApiFingerprint(auth)
  const attachments = Array.isArray(references) ? references : []
  const imagePrompt =
    'Generate an image based on this prompt: ' +
    prompt +
    '. Use the aspect ratio ' +
    aspectRatio +
    '.'
  return chatgptApiCompactObject({
    turn_attribution: { turn_trigger: 'composer' },
    action: 'next',
    is_do_not_remember: false,
    messages: [
      {
        id: apiTextUuid(),
        author: { role: 'user', name: null, metadata: {} },
        channel: null,
        create_time: Date.now() / 1000,
        content: chatgptApiMessageContent(attachments, imagePrompt),
        metadata: chatgptApiMessageMetadata(attachments),
        end_turn: null,
        recipient: 'all',
        status: 'finished_successfully',
        update_time: null,
        weight: 1,
      },
    ],
    parent_message_id: 'client-created-root',
    model,
    client_prepare_state: 'success',
    timezone_offset_min: Number(fingerprint.timezone_offset_min || 0),
    timezone: fingerprint.timezone || 'UTC',
    system_hints: ['picture_v2'],
    supported_encodings: ['v1'],
    client_contextual_info: chatgptApiTextClientContext(auth),
    thinking_effort: chatgptApiThinkingEffort(thinkingEffort),
    local_function_names: ['local.continue_in_work'],
  })
}

function chatgptApiTextClientContext(auth) {
  const fingerprint = chatgptApiFingerprint(auth)
  return chatgptApiCompactObject({
    app_name: 'chatgpt.com',
    app_surface: fingerprint.app_surface,
    has_web_push_capabilities: true,
    web_push_notification_permission: 'default',
  })
}

function chatgptApiThinkingEffort(value) {
  if (value === undefined || value === null || String(value).trim() === '') return undefined
  return String(value).trim()
}

function chatgptApiMessageContent(references, prompt) {
  if (!references.length) return { content_type: 'text', parts: [prompt] }
  return {
    content_type: 'multimodal_text',
    parts: [
      ...references.map((reference) => ({
        content_type: 'image_asset_pointer',
        asset_pointer: 'sediment://' + reference.file_id,
        size_bytes: reference.size,
        width: reference.width,
        height: reference.height,
      })),
      prompt,
    ],
  }
}

function chatgptApiAttachmentMetadata(reference) {
  return chatgptApiCompactObject({
    id: reference.file_id,
    size: reference.size,
    name: reference.name,
    mime_type: reference.mime_type,
    width: reference.width,
    height: reference.height,
    source: 'local',
    library_file_id: reference.library_file_id,
    is_big_paste: false,
  })
}

function chatgptApiMessageMetadata(references) {
  const metadata = {
    system_hints: ['picture_v2'],
  }
  if (references.length) metadata.attachments = references.map(chatgptApiAttachmentMetadata)
  return metadata
}

function chatgptApiEchoLogs(auth) {
  return '0,' + Math.max(0, Date.now() - Number(auth.runtime_started_at || Date.now()))
}

function chatgptApiUnique(values) {
  return values.filter((value, index) => values.indexOf(value) === index)
}

// 不把 Codex 的 high/xhigh 等档位降档映射为 ChatGPT 的固定值。
function chatgptApiResolveReasoning(model, requested, models, fallback) {
  const metadata = Array.isArray(models) ? models.find((item) => item.slug === model) : null
  let capability = fallback
  if (metadata?.configurable_thinking_effort === false) capability = { efforts: [] }
  else if (Array.isArray(metadata?.thinking_efforts)) {
    capability = {
      efforts: metadata.thinking_efforts.map((item) =>
        typeof item === 'string' ? item : item.thinking_effort
      ),
      default_effort: metadata.default_thinking_effort,
    }
  }
  const effort = String(requested || '').trim()
  if (effort && !capability?.efforts?.includes(effort)) {
    throw new Error('invalid_input: ' + model + ' 不支持思考深度 ' + effort)
  }
  return effort || capability?.default_effort || undefined
}

function chatgptApiTextReasoning(ctx, auth, request, modelCatalog) {
  let response = modelCatalog ? { body: modelCatalog } : undefined
  try {
    response ||= chatgptApiRequest(auth, 'GET', '/backend-api/models', {
      target_path: '/backend-api/models',
      target_route: '/backend-api/models',
      timeout_ms: 15000,
    })
  } catch (error) {
    if (/^(?:verification_required|LOGIN_EXPIRED):/.test(String(error?.message || ''))) throw error
    if (!ctx.model_capabilities?.reasoning) throw error
  }
  const selection = chatgptApiModelSelection(
    request.model,
    request.reasoning.effort,
    response?.body?.models,
    ctx.model_capabilities?.reasoning
  )
  request.model = selection.model
  request.reasoning.effort = selection.thinking_effort
}

// 一个型号入口包含即时与思考模式；网页请求 ID 的区别只在插件内处理。
function chatgptApiModelFamily(model) {
  const families = {
    'gpt-5-6': { instant: 'gpt-5-6-instant', thinking: 'gpt-5-6-thinking' },
    'gpt-5-6-mini': { instant: 'gpt-5-6-mini', thinking: 'gpt-5-6-t-mini', fixed: true },
  }
  return families[model]
}

function chatgptApiModelSelection(model, requested, models, fallback) {
  const family = chatgptApiModelFamily(model)
  let effort = requested
  let capability = fallback
  if (family) {
    effort = requested || fallback?.default_effort || 'standard'
    if (family.fixed && !['none', 'standard'].includes(effort)) {
      throw new Error('invalid_input: 当前模型只支持关闭或开启思考')
    }
    model = effort === 'none' ? family.instant : family.thinking
    if (effort === 'none' || family.fixed) {
      effort = undefined
      capability = { efforts: [] }
    }
  }
  if (Array.isArray(models) && !models.some((item) => item.slug === model)) {
    throw new Error('invalid_input: 当前 ChatGPT 账号不支持所选模型或思考模式，请重新选择')
  }
  return {
    model,
    thinking_effort: chatgptApiResolveReasoning(model, effort, models, capability),
  }
}

function chatgptApiDiscoveredReasoning(item, models) {
  const levels = (value) =>
    (value?.thinking_efforts || [])
      .map((level) => (typeof level === 'string' ? level : level.thinking_effort))
      .filter(Boolean)
  const family = chatgptApiModelFamily(item.slug)
  if (!family)
    return {
      efforts: item.configurable_thinking_effort === false ? [] : levels(item),
      default_effort: item.default_thinking_effort,
    }
  const instant = models.find((model) => model.slug === family.instant)
  const thinking = models.find((model) => model.slug === family.thinking)
  const efforts = [
    ...(instant ? ['none'] : []),
    ...(thinking ? (family.fixed ? ['standard'] : levels(thinking)) : []),
  ]
  const preferred = thinking?.default_thinking_effort || (thinking ? 'standard' : 'none')
  return { efforts, default_effort: efforts.includes(preferred) ? preferred : efforts[0] }
}
