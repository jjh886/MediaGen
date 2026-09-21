// 只复现会影响账号、模型和生图能力判断的启动请求。侧边栏、通知、埋点和静态资源不参与
// 生成协议，照搬它们既浪费带宽，也会制造并非真实用户操作的遥测数据。
function chatgptApiBootstrap(auth) {
  const offset = Number(chatgptApiFingerprint(auth).timezone_offset_min || 0)
  const requests = [
    ['/backend-api/me', '/backend-api/me'],
    ['/backend-api/accounts/optimized/check', '/backend-api/accounts/optimized/check'],
    [
      '/backend-api/accounts/check/v4-2023-04-27?timezone_offset_min=' + offset,
      '/backend-api/accounts/check/{version}',
    ],
    ['/backend-api/settings/user', '/backend-api/settings/user'],
    [
      '/backend-api/models?iim=false&is_gizmo=false&supports_model_picker_upgrade_presets=true',
      '/backend-api/models',
    ],
    ['/backend-api/images/bootstrap', '/backend-api/images/bootstrap'],
  ]
  const responses = {}
  for (const [path, route] of requests) {
    const targetPath = path.split('?')[0]
    responses[targetPath] = chatgptApiRequireOk(
      chatgptApiRequest(auth, 'GET', path, {
        target_path: targetPath,
        target_route: route,
        timeout_ms: 60000,
      }),
      '加载 ChatGPT 启动状态 ' + targetPath
    ).body
  }
  return responses
}
