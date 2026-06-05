function main(ctx) {
  let auth
  try {
    auth = codexEnsureFreshAuth(ctx)
  } catch (error) {
    return codexExpiredHealth(error)
  }

  let result = codexFetchAccountProfile(auth)
  if (!result.success && codexHttpStatus(result) === 401) {
    try {
      auth = codexRefreshAuth(ctx, auth, '账号检查返回 401')
      result = codexFetchAccountProfile(auth)
    } catch (error) {
      return codexExpiredHealth(error)
    }
  }

  return codexHealthFromHttpResult(result, auth)
}

function codexExpiredHealth(error) {
  return {
    logged_in: false,
    status: 'login_expired',
    reason: String((error && error.message) || error || 'Codex 登录已过期').replace(
      /^LOGIN_EXPIRED:\s*/,
      ''
    ),
  }
}
