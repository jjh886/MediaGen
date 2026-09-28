;(async () => {
  try {
    const token = JSON.parse(localStorage.getItem('userToken') || '{}').value
    if (!token) return JSON.stringify({ loggedIn: false })
    const response = await fetch('/api/v0/users/current', {
      headers: { authorization: 'Bearer ' + token },
      credentials: 'include',
    })
    const payload = await response.json()
    const user = payload?.data?.biz_data
    return JSON.stringify({
      loggedIn: response.ok && Boolean(user?.id),
      userId: String(user?.id || ''),
      username: user?.name || user?.mobile_number || user?.email || '',
      email: user?.mobile_number || user?.email || '',
    })
  } catch (_) {
    return JSON.stringify({ loggedIn: false })
  }
})()
