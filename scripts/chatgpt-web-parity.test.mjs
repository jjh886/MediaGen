import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, resolve, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import vm from 'node:vm'
import { test } from 'node:test'

const plugins = resolve(dirname(fileURLToPath(import.meta.url)), '../plugins')
const clone = (value) => JSON.parse(JSON.stringify(value))

test('ChatGPT 正式文本请求对齐网页实测的 prepare 成功状态和消息结构', () => {
  const ctx = vm.createContext({})
  for (const file of ['poll_retry.js', 'auth.js', 'conversation.js', 'text_conversation.js'])
    vm.runInContext(readFileSync(join(plugins, 'chatgpt/scripts', file), 'utf8'), ctx)
  const body = clone(
    ctx.chatgptApiTextConversationPayload(
      {
        auth: {
          fingerprint: {
            timezone: 'Asia/Shanghai',
            timezone_offset_min: -480,
            app_surface: 'codex_browser',
          },
        },
      },
      '协议对照测试，请只回复：收到。',
      {
        user_message_id: 'test-user-message',
        parent_message_id: 'client-created-root',
        model: 'gpt-6-thinking',
      }
    )
  )
  delete body.messages[0].create_time
  assert.deepEqual(body, {
    turn_attribution: { turn_trigger: 'composer' },
    action: 'next',
    is_do_not_remember: false,
    model: 'gpt-6-thinking',
    parent_message_id: 'client-created-root',
    timezone: 'Asia/Shanghai',
    timezone_offset_min: -480,
    client_contextual_info: {
      app_name: 'chatgpt.com',
      app_surface: 'codex_browser',
      has_web_push_capabilities: true,
      web_push_notification_permission: 'default',
    },
    local_function_names: ['local.continue_in_work'],
    supported_encodings: ['v1'],
    genui_state_snapshots: [],
    client_prepare_state: 'success',
    messages: [
      {
        author: { metadata: {}, name: null, role: 'user' },
        channel: null,
        content: { content_type: 'text', parts: ['协议对照测试，请只回复：收到。'] },
        end_turn: null,
        id: 'test-user-message',
        metadata: {},
        recipient: 'all',
        status: 'finished_successfully',
        update_time: null,
        weight: 1,
      },
    ],
  })
})

test('ChatGPT 视频上传和最终附件引用与网页抓包一致', () => {
  const ctx = vm.createContext({
    host: {
      call: () => {
        throw Error('意外的宿主调用')
      },
    },
  })
  for (const file of [
    'poll_retry.js',
    'auth.js',
    'network_profile.js',
    'conversation.js',
    'text_conversation.js',
    'reference_upload.js',
  ])
    vm.runInContext(readFileSync(join(plugins, 'chatgpt/scripts', file), 'utf8'), ctx)
  const requests = []
  ctx.apiTextMaterializeAttachment = () => ({ path: '/fixture/episode.mp4', size: 2550180 })
  ctx.chatgptApiUploadSignedBlob = (_auth, url, file) => {
    assert.equal(url, 'https://upload.example.test/fixture')
    assert.equal(file.path, '/fixture/episode.mp4')
    assert.equal(file.use_case, 'ace_upload')
  }
  ctx.chatgptApiRequest = (_auth, method, path, options) => {
    requests.push(clone({ method, path, body: options.body }))
    if (path === '/backend-api/files')
      return {
        ok: true,
        body: { file_id: 'file-test', upload_url: 'https://upload.example.test/fixture' },
      }
    if (path === '/backend-api/files/process_upload_stream')
      return {
        ok: true,
        body_text: JSON.stringify({
          file_id: 'file-test',
          event: 'file.processing.completed',
          extra: {
            metadata_object_id: 'libfile-test',
            library_file_name: 'episode(1).mp4',
            mime_type: 'video/mp4',
          },
        }),
      }
    assert.equal(path, '/backend-api/files/upload_reservations')
    return { ok: true, body: {} }
  }
  const auth = { auth: { fingerprint: { timezone_offset_min: -480 } } }
  const attachments = ctx.chatgptApiUploadTextAttachments({ workspace_dir: '/fixture' }, auth, [
    {
      text: '识别视频',
      attachments: [{ name: 'episode.mp4', mime_type: 'video/mp4', size: 2550180 }],
    },
  ])
  assert.deepEqual(requests, [
    {
      method: 'POST',
      path: '/backend-api/files/upload_reservations',
      body: {
        intended_use_case: 'my_files',
        entry_surface: 'chat_composer',
        requires_gizmo_id: false,
        store_in_library: true,
        library_persistence_mode: 'opportunistic',
      },
    },
    {
      method: 'POST',
      path: '/backend-api/files',
      body: {
        entry_surface: 'chat_composer',
        file_name: 'episode.mp4',
        file_size: 2550180,
        client_resolved_mime_type: 'video/mp4',
        mime_resolution_source: 'filename_extension',
        mime_type: 'video/mp4',
        reset_rate_limits: false,
        supports_direct_azure_multipart: true,
        timezone_offset_min: -480,
        use_case: 'ace_upload',
        store_in_library: true,
        library_persistence_mode: 'opportunistic',
      },
    },
    {
      method: 'POST',
      path: '/backend-api/files/process_upload_stream',
      body: {
        entry_surface: 'chat_composer',
        file_id: 'file-test',
        file_name: 'episode.mp4',
        index_for_retrieval: false,
        library_persistence_mode: 'opportunistic',
        metadata: { store_in_library: true, is_project_thread: false },
        mime_type: 'video/mp4',
        use_case: 'ace_upload',
      },
    },
  ])
  const payload = clone(
    ctx.chatgptApiTextConversationPayload(
      auth,
      '识别视频',
      {
        user_message_id: 'user-test',
        parent_message_id: 'client-created-root',
        model: 'gpt-5-6-thinking',
        thinking_effort: 'standard',
      },
      attachments
    )
  )
  assert.equal(payload.client_prepare_state, 'success')
  assert.deepEqual(payload.messages[0].content, { content_type: 'text', parts: ['识别视频'] })
  assert.deepEqual(payload.messages[0].metadata, {
    file_upload_slot_prefetch_attribution: {
      actual_path: 'legacy',
      reason: 'direct_library_multipart_preserved',
    },
    attachments: [
      {
        id: 'file-test',
        library_file_id: 'libfile-test',
        mime_type: 'video/mp4',
        name: 'episode(1).mp4',
        size: 2550180,
      },
    ],
  })
})
