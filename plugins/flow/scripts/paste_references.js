// Flow: 通过 paste 事件把多张参考图注入到 prompt 输入框，保证 ingredients 顺序。
//
// 顺序错乱根因（旧版并发派发）：
//   并发派发多张 paste 后，Flow 前端按 uploadImage XHR 响应先后把图片写入
//   ingredients，而响应先后 ≠ paste 先后，网络/服务端抖动时顺序就乱了。
//
// 本实现（串行 + XHR 信号）：
//   1. 所有 data URL 并发 fetch 成 File；
//   2. 一张一张 paste，每张 paste 后等 setup_interceptor.js 拦截到的
//      window.__mediagen_uploaded_refs 长度 +1 作为"该张上传完成"的信号；
//   3. XHR 信号超时则降级为 DOM 百分比消失兜底；
//   4. 全部派发完毕后返回。
//
// 相比 FlowForge 的串行 + DOM 百分比轮询，用拦截到的 XHR 响应判定完成更精准、
// 更快，且能按张定位失败。
//
// 前提：调用本脚本前，setup_interceptor.js 已注入，拦截器在 uploadImage 成功
// 响应时会 push 到 window.__mediagen_uploaded_refs。
//
// 参考图来源：window.__mediagen_params.ref_image_dataurls （data URL 数组）
// 返回：JSON 字符串 { pasted, total, uploaded, details: [...] }
;(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

  const getTextbox = () =>
    document.querySelector('div[role="textbox"][contenteditable="true"]') ||
    document.querySelector('textarea')

  // 把一个 File 以 paste 事件打进 textbox
  const pasteFile = (textbox, file) => {
    const dt = new DataTransfer()
    dt.items.add(file)

    let event = new ClipboardEvent('paste', {
      bubbles: true,
      cancelable: true,
      clipboardData: dt,
    })
    // main world 下 clipboardData 多半是 null，强制覆盖一次。
    if (!event.clipboardData || event.clipboardData !== dt) {
      try {
        Object.defineProperty(event, 'clipboardData', { value: dt, writable: false })
      } catch (_) {
        // 部分浏览器 ClipboardEvent.clipboardData 不可覆盖，降级用自定义 Event
        event = new Event('paste', { bubbles: true, cancelable: true })
        Object.defineProperty(event, 'clipboardData', { value: dt, writable: false })
      }
    }
    textbox.dispatchEvent(event)
  }

  // 主信号：拦截器 push 的 __mediagen_uploaded_refs 长度达到 target
  const waitUploadedCount = async (target, timeoutMs) => {
    const deadline = Date.now() + timeoutMs
    while (Date.now() < deadline) {
      const arr = window.__mediagen_uploaded_refs || []
      if (arr.length >= target) return true
      await sleep(200)
    }
    return false
  }

  // 兜底信号：页面上"xx%"进度小 div 全部消失
  const waitPercentGone = async (timeoutMs) => {
    const deadline = Date.now() + timeoutMs
    await sleep(1000)
    while (Date.now() < deadline) {
      let hasPercent = false
      for (const div of document.querySelectorAll('div')) {
        const text = (div.textContent || '').trim()
        if (!/^\d{1,3}%$/.test(text)) continue
        const rect = div.getBoundingClientRect()
        if (rect.width > 0 && rect.width < 150 && rect.height < 50) {
          hasPercent = true
          break
        }
      }
      if (!hasPercent) return true
      await sleep(500)
    }
    return false
  }

  try {
    const params = window.__mediagen_params || {}
    const urls = Array.isArray(params.ref_image_dataurls) ? params.ref_image_dataurls : []
    if (urls.length === 0) {
      return JSON.stringify({ pasted: 0, total: 0, skipped: true, reason: '无参考图' })
    }

    // 1) 并发 fetch 所有参考图为 File，单张失败不阻塞其它
    const fetched = await Promise.all(
      urls.map(async (url, i) => {
        try {
          const resp = await fetch(url)
          const blob = await resp.blob()
          const mime = blob.type || 'image/png'
          const ext = mime.includes('jpeg') ? 'jpg' : mime.includes('webp') ? 'webp' : 'png'
          const ts = Date.now()
          return {
            index: i,
            file: new File([blob], `ref_${i}_${ts}.${ext}`, { type: mime }),
            mime,
          }
        } catch (err) {
          return { index: i, error: String(err?.message || err) }
        }
      })
    )

    const details = fetched.map((r) => ({ index: r.index, mime: r.mime, error: r.error }))
    const ready = fetched.filter((r) => r.file)
    if (ready.length === 0) {
      return JSON.stringify({ pasted: 0, total: urls.length, uploaded: false, details })
    }

    // 2) 重置拦截器累加器作为串行完成基线
    //    cleanup.js 不依赖其内容（全量 DOM 扫描删图），重置安全
    if (!Array.isArray(window.__mediagen_uploaded_refs)) {
      window.__mediagen_uploaded_refs = []
    }
    window.__mediagen_uploaded_refs.length = 0

    // 3) 串行 paste：一张完成再派发下一张，保证 ingredients 顺序
    let pasted = 0
    let needFocus = true
    for (let i = 0; i < ready.length; i++) {
      const item = ready[i]
      const textbox = getTextbox()
      if (!textbox) {
        details[item.index].error = '未找到 prompt textbox'
        break
      }
      if (needFocus) {
        textbox.focus()
        if (typeof textbox.click === 'function') textbox.click()
        await sleep(200)
        needFocus = false
      }

      try {
        pasteFile(textbox, item.file)
        pasted += 1
        details[item.index].pasted = true
      } catch (err) {
        details[item.index].error = String(err?.message || err)
        break
      }

      // 主信号：拦截到的 uploadImage 响应数量达到 i+1
      const target = i + 1
      const okByXhr = await waitUploadedCount(target, 25000)
      if (okByXhr) {
        details[item.index].confirmedBy = 'xhr'
      } else {
        // 兜底：DOM 百分比消失
        const okByDom = await waitPercentGone(20000)
        details[item.index].confirmedBy = okByDom ? 'dom' : 'timeout'
        if (!okByDom) {
          details[item.index].error = '上传完成信号超时'
          // 超时继续下一张：已 paste 的图大概率前端已开始上传，中止反而浪费
        }
      }

      // 给 Slate 一点消化时间再派发下一张，避免光标/选区抖动
      if (i < ready.length - 1) await sleep(300)
    }

    const uploaded = (window.__mediagen_uploaded_refs?.length || 0) >= ready.length

    return JSON.stringify({ pasted, total: urls.length, uploaded, details })
  } catch (e) {
    return JSON.stringify({ error: String(e?.message || e) })
  }
})()
