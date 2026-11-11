function main(ctx) {
  try {
    const session = geminiInitialize(apiSessionRead(ctx))
    const models = geminiCatalog(session)
    if (!models.length) throw new Error('AUTH_INCONCLUSIVE: Google 未返回账号可用模型')
    const account = geminiAccountProfile(session.auth, session.user_status)
    let quota
    try {
      quota = geminiQuotaWindows(
        geminiRpc(session, 'VxUbXb', []),
        geminiRpc(session, 'aPya6c', []),
        geminiRpc(session, 'qpEbW', [
          [
            [1, 7],
            [1, 5],
          ],
        ])
      )
    } catch (_) {
      quota = undefined
    }
    return {
      logged_in: true,
      status: 'available',
      account,
      reason: quota
        ? '身份已验证；额度状态来自 Gemini，总量未公开时显示未知'
        : '身份已验证；额度暂时无法读取，保留原数据',
      ...(quota ? { quota_windows: quota } : {}),
    }
  } catch (error) {
    if (String(error.message).startsWith('verification_required:'))
      return { logged_in: null, status: 'verification_required', reason: error.message }
    return { ...apiSessionHealthError(error), reason: error.message }
  }
}
