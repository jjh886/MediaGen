// SDK 保持在本次插件任务的 JS 上下文中；网络仍由受权限约束的 HTTP 原子执行。
function doubaoSecurityTransport(input, Headers) {
  const timers = new Map()
  let timerId = 0
  let expected = null
  let pending = null
  let streamingBody = null
  let failure = null
  function schedule(callback, delay, interval, args) {
    if (typeof callback !== 'function') throw new Error('安全 SDK 定时回调无效')
    const id = ++timerId
    const milliseconds = Math.max(0, Number(delay) || 0)
    timers.set(id, { callback, due: Date.now() + milliseconds, interval, milliseconds, args })
    return id
  }
  function response(value, url, body) {
    if (!body) {
      if (typeof value.body_text === 'string') body = Promise.resolve(value.body_text)
      else {
        body = new Promise((resolve, reject) => {
          streamingBody = { resolve, reject }
        })
      }
    }
    const text = () => body
    return {
      url,
      status: value.status,
      ok: value.ok,
      headers: new Headers(value.headers),
      text,
      json: () => text().then((body) => JSON.parse(body)),
      clone: () => response(value, url, body),
    }
  }
  function send(request) {
    const value = input.send(request)
    if (!value || !Number.isInteger(value.status))
      throw new Error('invalid_response: 安全 SDK 请求没有真实 HTTP 响应')
    return value
  }
  function fetch(url, options = {}) {
    const request = {
      url: typeof url === 'string' ? url : url.url,
      method: options.method || url.method || 'GET',
      headers: new Headers(options.headers || url.headers).values,
      body: options.body === undefined ? url.body : options.body,
    }
    if (expected && request.url.split('?')[0] === expected.split('?')[0]) {
      if (pending) throw new Error('安全 SDK 同时产生了多个业务请求')
      return new Promise((resolve, reject) => {
        pending = { request, resolve, reject }
      })
    }
    return Promise.resolve().then(() => {
      try {
        return response(send(request), request.url)
      } catch (error) {
        failure = error
        throw error
      }
    })
  }
  class XMLHttpRequest {
    constructor() {
      this.headers = {}
      this.listeners = new Map()
      this.readyState = 0
      this.status = 0
      this.withCredentials = false
      this.aborted = false
    }
    open(method, url, async = true) {
      this.method = method
      this.url = url
      this.async = async
      this.readyState = 1
    }
    setRequestHeader(name, value) {
      this.headers[name] = String(value)
    }
    addEventListener(name, callback) {
      const values = this.listeners.get(name) || new Set()
      values.add(callback)
      this.listeners.set(name, values)
    }
    removeEventListener(name, callback) {
      this.listeners.get(name)?.delete(callback)
    }
    notify(name) {
      const event = { type: name, target: this, currentTarget: this }
      this['on' + name]?.call(this, event)
      for (const callback of this.listeners.get(name) || []) callback.call(this, event)
    }
    getAllResponseHeaders() {
      return Object.entries(this.responseHeaders || {})
        .map(([name, value]) => name + ': ' + value + '\r\n')
        .join('')
    }
    getResponseHeader(name) {
      return new Headers(this.responseHeaders).get(name)
    }
    abort() {
      this.aborted = true
    }
    send(body) {
      const execute = () => {
        if (this.aborted) return
        try {
          const result = send({
            url: this.url,
            method: this.method,
            headers: this.headers,
            body,
            with_credentials: this.withCredentials,
          })
          this.status = result.status
          this.responseURL = this.url
          this.responseHeaders = result.headers
          this.readyState = 2
          this.notify('readystatechange')
          this.responseText = result.body_text || ''
          this.response =
            this.responseType === 'json' ? JSON.parse(this.responseText) : this.responseText
          this.readyState = 4
          this.notify('readystatechange')
          this.notify('load')
          this.notify('loadend')
        } catch (error) {
          failure = error
          this.status = 0
          this.readyState = 4
          this.notify('error')
          this.notify('loadend')
        }
      }
      if (this.async) Promise.resolve().then(execute)
      else execute()
    }
  }
  function tick() {
    input.drain_jobs()
    let executed = 0
    for (const [id, timer] of timers) {
      if (timer.due > Date.now()) continue
      if (++executed > 128) throw new Error('安全 SDK 定时任务超过执行上限')
      if (timer.interval) timer.due = Date.now() + Math.max(1, timer.milliseconds)
      else timers.delete(id)
      timer.callback(...timer.args)
      input.drain_jobs()
    }
    if (failure) throw failure
  }
  return {
    fetch,
    XMLHttpRequest,
    setTimeout: (callback, delay, ...args) => schedule(callback, delay, false, args),
    setInterval: (callback, delay, ...args) => schedule(callback, delay, true, args),
    clearTimeout: (id) => timers.delete(id),
    clearInterval: (id) => timers.delete(id),
    requestAnimationFrame: (callback) =>
      schedule(() => callback(Date.now() - input.started_at), 16, false, []),
    cancelAnimationFrame: (id) => timers.delete(id),
    tick,
    prepare(url) {
      tick()
      if (pending) throw new Error('安全 SDK 上一次请求尚未收到响应')
      expected = url
    },
    signed() {
      if (!pending) throw new Error('安全 SDK 未产生业务请求')
      expected = null
      return pending.request
    },
    complete(value) {
      if (!pending) throw new Error('安全 SDK 没有等待中的业务请求')
      const current = pending
      pending = null
      current.resolve(response(value, current.request.url))
      tick()
    },
    finish(value) {
      if (!streamingBody) return
      if (typeof value.body_text !== 'string')
        throw new Error('invalid_response: 安全 SDK 缺少完整响应正文')
      streamingBody.resolve(value.body_text)
      streamingBody = null
      tick()
    },
  }
}
