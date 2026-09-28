;(async () => {
  try {
    const context = globalThis.__pluginLoginContext
    if (context?.requires_manual_verification) {
      let state = globalThis.__doubaoLoginVerification
      if (!state && context.decision && typeof window.verifySDK?.autoRender === 'function') {
        state = globalThis.__doubaoLoginVerification = { completed: false }
        const complete = () => {
          state.completed = true
        }
        const options = {
          commonOptions: { aid: 497858 },
          verify_data: context.decision,
          captchaOptions: { showMode: 'mask', successCb: complete, closeCb: () => {} },
          secondVerifyWebOptions: { callBack: complete, closeCallBack: () => {} },
        }
        try {
          if (String(context.decision.code) === '10000') {
            await window.verifySDK.renderCaptcha(options)
          } else {
            await window.verifySDK.autoRender(options)
          }
        } catch (error) {
          delete globalThis.__doubaoLoginVerification
          throw error
        }
      }
      if (!state?.completed) return JSON.stringify({ loggedIn: false })
    }
    const url = doubaoBrowserAccountUrl(
      performance.getEntriesByType('resource').map((entry) => entry.name)
    )
    const response = await fetch(url.href, {
      credentials: 'include',
      headers: { 'content-type': 'application/json' },
    })
    const body = await response.json()
    const user = body.data
    return JSON.stringify({
      loggedIn: response.ok && Boolean(user?.user_id_str || user?.user_id),
      userId: String(user?.user_id_str || user?.user_id || ''),
      username: user?.screen_name || user?.name || user?.mobile || user?.email || '',
      email: user?.mobile || user?.email || '',
      avatarUrl: user?.avatar_url || '',
    })
  } catch (_) {
    return JSON.stringify({ loggedIn: false })
  }
})()
