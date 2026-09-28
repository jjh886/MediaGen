;(async () => {
  const token = JSON.parse(localStorage.getItem('userToken') || '{}').value
  if (!token || typeof token !== 'string') throw new Error('DeepSeek 尚未完成登录')
  const response = await fetch('/api/v0/users/current', {
    headers: { authorization: 'Bearer ' + token },
    credentials: 'include',
  })
  const payload = await response.json()
  const user = payload?.data?.biz_data
  if (!response.ok || !user?.id) throw new Error('DeepSeek 登录状态尚未确认')
  return JSON.stringify({
    access_token: token,
    user: { id: user.id },
    device_id: localStorage.getItem('deepseek-device-id:chat') || '',
    fingerprint: await apiLoginFingerprint(),
  })
})()
