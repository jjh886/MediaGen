// 提交点击后的确认与补点。wait_submit_ready 负责标记按钮，本脚本确认生成请求真的发出。
;(async () => {
  const SK = Symbol.for('_jimeng')
  const state = (window[SK] = window[SK] || {})
  const marker = '[data-frame64-jimeng-submit="true"]'
  const initialWaitMs = Number(state.submit_confirm_initial_wait_ms || 900)
  const retryWaitMs = Number(state.submit_confirm_retry_wait_ms || 1200)
  const retryAttempts = Math.max(0, Number(state.submit_confirm_attempts ?? 2))
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

  const classOf = (el) => String(el?.className || '')
  const textOf = (el) =>
    [el?.textContent, el?.getAttribute?.('aria-label'), el?.getAttribute?.('title')]
      .join(' ')
      .replace(/\s+/g, ' ')
      .trim()

  const isVisible = (el) => {
    if (!el) return false
    const rect = el.getBoundingClientRect?.()
    if (!rect || rect.width <= 0 || rect.height <= 0) return false
    const style = window.getComputedStyle?.(el)
    return (
      !style || (style.display !== 'none' && style.visibility !== 'hidden' && style.opacity !== '0')
    )
  }

  const isDisabled = (button) => {
    const cls = classOf(button)
    return (
      !!button?.disabled ||
      button?.getAttribute?.('aria-disabled') === 'true' ||
      /disabled|lv-btn-disabled/.test(cls)
    )
  }

  const hasGenerateFetch = () =>
    Array.isArray(state.fetchLog) &&
    state.fetchLog.some((item) => String(item?.url || '').includes('/aigc_draft/generate'))

  const generateRequestStarted = () => {
    return !!state.submit_id || hasGenerateFetch() || Number(state.generate_request_count || 0) > 0
  }

  const scoreButton = (button) => {
    if (!isVisible(button)) return -1
    if (
      button.closest?.('[role="dialog"], [aria-modal="true"], [class*="modal"], [class*="preview"]')
    )
      return -1
    const text = textOf(button)
    if (/上传|添加|参考|删除|做同款|升级会员|会员得积分/.test(text)) return -1
    const cls = classOf(button)
    let score = 0
    if (/submit-button/.test(cls)) score += 120
    if (/lv-btn-primary/.test(cls)) score += 60
    if (/shape-circle|icon-only/.test(cls)) score += 30
    if (button.closest?.('[class*="toolbar-actions"]')) score += 120
    if (button.closest?.('[class*="collapsed-submit-button-container"]')) score += 80
    return score
  }

  const findSubmitButton = () => {
    const marked = document.querySelector(marker)
    if (marked && !isDisabled(marked)) return marked
    const selectors = [
      '[class*="toolbar-actions"] button',
      '[class*="collapsed-submit-button-container"] button',
      'button[class*="submit-button"]',
      'button.lv-btn-primary',
    ].join(', ')
    return (
      [...document.querySelectorAll(selectors)]
        .map((button) => ({ button, score: scoreButton(button), disabled: isDisabled(button) }))
        .filter((item) => item.score > 0 && !item.disabled)
        .sort((a, b) => b.score - a.score)[0]?.button || null
    )
  }

  const eventBaseFor = (button) => {
    const view = button.ownerDocument?.defaultView || window
    const rect = button.getBoundingClientRect?.() || {}
    return {
      bubbles: true,
      cancelable: true,
      view,
      clientX: Math.round((rect.left || 0) + (rect.width || 0) / 2),
      clientY: Math.round((rect.top || 0) + (rect.height || 0) / 2),
    }
  }

  const makePointerEvent = (view, type, base) => {
    try {
      return view.PointerEvent ? new view.PointerEvent(type, base) : null
    } catch (_) {
      return null
    }
  }

  const makeMouseEvent = (view, type, base) => {
    try {
      return new view.MouseEvent(type, base)
    } catch (_) {
      return new view.Event(type, { bubbles: true, cancelable: true })
    }
  }

  const makeEvent = (view, type, base) =>
    type.startsWith('pointer')
      ? makePointerEvent(view, type, base) || makeMouseEvent(view, type, base)
      : makeMouseEvent(view, type, base)

  const dispatchPointerSequence = (button, view, base) => {
    for (const type of [
      'pointerover',
      'mouseover',
      'pointermove',
      'mousemove',
      'pointerdown',
      'mousedown',
      'pointerup',
      'mouseup',
    ]) {
      button.dispatchEvent(makeEvent(view, type, base))
    }
  }

  const clickButton = (button) => {
    button.scrollIntoView?.({ block: 'center', inline: 'center' })
    button.focus?.()
    const base = eventBaseFor(button)
    const view = base.view
    dispatchPointerSequence(button, view, base)
    try {
      button.click()
    } catch (_) {
      button.dispatchEvent(makeEvent(view, 'click', base))
    }
  }

  const finish = (extra = {}) => {
    const started = generateRequestStarted()
    state.submit_confirmed = started
    state.submit_confirmation = {
      confirmed: started,
      submit_id: state.submit_id || '',
      generate_request_count: Number(state.generate_request_count || 0),
      generate_request_started_at: Number(state.generate_request_started_at || 0),
      click_attempts: Number(state.submit_click_attempts || 0),
      at: Date.now(),
      ...extra,
    }
    return JSON.stringify(state.submit_confirmation)
  }

  try {
    await sleep(initialWaitMs)
    if (generateRequestStarted()) return finish({ reason: 'submit_request_seen' })

    for (let attempt = 0; attempt < retryAttempts; attempt += 1) {
      const button = findSubmitButton()
      if (!button) return finish({ reason: 'submit_button_missing' })
      if (isDisabled(button)) return finish({ reason: 'submit_button_disabled' })

      state.submit_click_attempts = Number(state.submit_click_attempts || 0) + 1
      state.submit_last_click_retry_at = Date.now()
      clickButton(button)
      await sleep(retryWaitMs)
      if (generateRequestStarted()) return finish({ reason: 'retry_click_started_submit' })
    }

    return finish({ reason: 'retry_click_no_submit_request' })
  } catch (e) {
    return finish({ reason: 'submit_confirm_error', error: String(e?.message || e) })
  }
})()
