function deepseekPow(session, targetPath) {
  const { challenge } = deepseekRequest(session, '/chat/create_pow_challenge', {
    target_path: targetPath,
  })
  if (!challenge || challenge.algorithm !== 'DeepSeekHashV1') {
    throw new Error('unsupported_challenge: DeepSeek 计算挑战算法不受支持')
  }
  const expire = challenge.expire_at ?? challenge.expireAt
  const prefix = challenge.salt + '_' + expire + '_'
  if (
    !/^[\x20-\x7e]+$/.test(prefix + challenge.challenge) ||
    prefix.length > 512 ||
    !Number.isFinite(challenge.difficulty) ||
    challenge.difficulty <= 0
  ) {
    throw new Error('invalid_response: DeepSeek 计算挑战参数无效')
  }
  const module = apiSessionRequireOk(
    host.call('http.request', {
      method: 'GET',
      url: 'https://fe-static.deepseek.com/chat/static/sha3_wasm_bg.7b9ca65ddd.wasm',
      response_body_format: 'base64',
      proxy: 'system',
      timeout_ms: 60000,
    }),
    '获取计算模块失败'
  )
  const result = host.call('crypto.wasm_execute', {
    module_base64: module.body_base64,
    fuel: 10000000000,
    operations: [
      { op: 'call', export: '__wbindgen_add_to_stack_pointer', args: [-16], save_as: 'stack' },
      {
        op: 'call',
        export: '__wbindgen_export_0',
        args: [challenge.challenge.length, 1],
        save_as: 'challenge',
      },
      { op: 'write_utf8', offset: { result: 'challenge' }, text: challenge.challenge },
      { op: 'call', export: '__wbindgen_export_0', args: [prefix.length, 1], save_as: 'prefix' },
      { op: 'write_utf8', offset: { result: 'prefix' }, text: prefix },
      {
        op: 'call',
        export: 'wasm_solve',
        args: [
          { result: 'stack' },
          { result: 'challenge' },
          challenge.challenge.length,
          { result: 'prefix' },
          prefix.length,
          challenge.difficulty,
        ],
      },
      { op: 'read_i32', offset: { result: 'stack' }, save_as: 'ok' },
      { op: 'read_f64', offset: { result: 'stack' }, add: 8, save_as: 'answer' },
    ],
  })
  if (!result.ok || !Number.isSafeInteger(result.answer) || result.answer < 0) {
    throw new Error('challenge_failed: DeepSeek 计算挑战未完成')
  }
  return host.call('crypto.base64_encode_utf8', {
    text: JSON.stringify({
      algorithm: challenge.algorithm,
      challenge: challenge.challenge,
      salt: challenge.salt,
      answer: result.answer,
      signature: challenge.signature,
      target_path: targetPath,
    }),
  }).value
}
