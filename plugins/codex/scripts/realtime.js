function main(ctx) {
  const auth = codexEnsureFreshAuth(ctx)
  const app = String(ctx.platform_config?.application_path || '/Applications/ChatGPT.app')
  const device = host.call('process.run', {
    command: app + '/Contents/Resources/cua_node/bin/node',
    args: [
      ctx.scripts_dir + '/device_attestation.cjs',
      app + '/Contents/Resources/native/devicecheck.node',
      app + '/Contents/Info.plist',
      ctx.request_id,
    ],
    timeout_ms: 15000,
  })
  if (!device.ok)
    throw new Error(
      'attestation_unavailable: 无法生成设备证明，需要 Apple Silicon macOS 和官方 ChatGPT 应用'
    )
  let attestation
  try {
    attestation = JSON.parse(device.stdout)
  } catch (_) {
    throw new Error('attestation_unavailable: 设备证明格式无效')
  }
  if (attestation.v !== 1 || typeof attestation.t !== 'string')
    throw new Error('attestation_unavailable: 设备证明无效')
  const headers = codexUpstreamHeaders(
    codexAccessToken(auth),
    codexAccountId(auth),
    'application/sdp'
  )
  delete headers['openai-beta']
  headers['openai-alpha'] = 'quicksilver=v2'
  headers['x-oai-attestation'] = JSON.stringify(attestation)
  headers['session-id'] = ctx.request_id
  headers['thread-id'] = ctx.request_id
  const result = host.call('http.request', {
    method: 'POST',
    url: 'https://chatgpt.com/backend-api/codex/realtime/calls?intent=quicksilver&architecture=avas',
    headers,
    fingerprint: 'none',
    follow_redirects: false,
    body: ctx.request,
    timeout_ms: 60000,
  })
  if (!result.ok) throw new Error('HTTP ' + result.status + ': 实时会话创建失败')
  const location = String(result.headers?.location || '').split('?')[0]
  const callId = location.slice(location.lastIndexOf('/') + 1)
  if (!/^[a-zA-Z0-9_-]{1,256}$/.test(callId)) throw new Error('实时接口没有返回有效的通话 ID')
  if (typeof result.body_text !== 'string' || !result.body_text.trim().startsWith('v=0'))
    throw new Error('实时接口没有返回有效的 SDP')
  headers.accept = '*/*'
  delete headers['content-type']
  return {
    success: true,
    call_id: callId,
    sdp: result.body_text,
    connection: {
      url: 'wss://chatgpt.com/backend-api/codex/' + callId,
      headers,
      fingerprint: 'none',
    },
    close_event: { type: 'session.close' },
    terminal_events: ['session.closed', 'session.ended'],
  }
}
