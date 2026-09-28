function doubaoBrowserAccountUrl(resources) {
  const observed = resources
    .slice()
    .reverse()
    .find((url) => url.startsWith('https://accounts.doubao.com/passport/account/info/v2/?'))
  const url = new URL(observed || 'https://accounts.doubao.com/passport/account/info/v2/')
  if (!observed) {
    // 已登录页面可直接使用服务端注入的用户信息，不一定再次请求 account/info。
    const common = resources
      .slice()
      .reverse()
      .find((entry) => entry.startsWith('https://www.doubao.com/') && entry.includes('web_id='))
    if (!common) throw new Error('豆包账号协议尚未初始化，请稍后重试')
    const query = new URL(common).searchParams
    for (const key of [
      'pc_version',
      'version_code',
      'language',
      'device_platform',
      'aid',
      'real_aid',
      'pkg_type',
      'use-olympus-account',
      'region',
      'sys_region',
    ]) {
      if (!query.has(key)) throw new Error('豆包账号参数缺失：' + key)
      url.searchParams.set(key, query.get(key))
    }
    // 当前网页账号初始化协议的固定字段；版本随平台适配更新。
    url.searchParams.set('account_sdk_source', 'web')
    url.searchParams.set('sdk_version', '2.2.10')
    url.searchParams.set('doubao_from', 'inject_html')
  }
  url.searchParams.delete('msToken')
  url.searchParams.delete('a_bogus')
  return url
}
