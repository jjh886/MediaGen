function main(ctx) {
  try {
    const session = geminiInitialize(apiSessionRead(ctx))
    const models = geminiCatalog(session)
    if (!models.length) throw new Error('AUTH_INCONCLUSIVE: Google 未返回账号可用模型')
    return {
      logged_in: true,
      status: 'available',
      account: { email: session.auth.email || '', name: session.auth.email || 'Google Gemini' },
      reason: '登录有效，发现 ' + models.length + ' 个文本模型；上游未提供固定剩余额度',
      quota_windows: [],
    }
  } catch (error) {
    if (String(error.message).startsWith('verification_required:'))
      return { logged_in: null, status: 'verification_required', reason: error.message }
    return { ...apiSessionHealthError(error), reason: error.message }
  }
}
