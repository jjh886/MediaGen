function main(ctx) {
  try {
    const session = geminiInitialize(apiSessionRead(ctx))
    const models = geminiCatalog(session)
    if (!models.length) throw new Error('AUTH_INCONCLUSIVE: Google 未返回账号可用模型')
    const account = geminiAccountProfile(session.auth, session.user_status)
    try {
      Object.assign(
        account,
        geminiAccountDetails(session.auth, geminiRpc(session, 'o30O0e', GEMINI_PROFILE_ARGS))
      )
    } catch (_) {
      /* 头像读取失败时保留原资料。 */
    }
    const quota = geminiUsageWindows(
      geminiRequest(session, geminiBase(session) + geminiUsagePath(), {
        authorize: false,
        headers: {
          accept: 'application/json',
          referer: geminiBase(session) + '/usage',
          'sec-fetch-site': 'same-origin',
          'sec-fetch-mode': 'cors',
          'sec-fetch-dest': 'empty',
        },
      }).body_text
    )
    return {
      logged_in: true,
      status: 'available',
      account,
      reason: '身份已验证；当前用量来自 Gemini 网页统一额度',
      quota_windows: quota,
    }
  } catch (error) {
    if (String(error.message).startsWith('verification_required:'))
      return { logged_in: null, status: 'verification_required', reason: error.message }
    return { ...apiSessionHealthError(error), reason: error.message }
  }
}
