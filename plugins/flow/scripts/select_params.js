// Flow: 选择比例/数量/模型
// 输入: window.__mediagen_params = { model_display_name, ratio, count }
// 所有点击都用坐标计算 + CDP 可信鼠标事件（通过 window.__mediagen_cdp_click 回调）
// 如果 CDP 点击不可用，降级为 dispatchEvent 模拟
;(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
  const params = window.__mediagen_params || {}
  const targetModel = params.model_display_name || 'Nano Banana Pro'
  const normalizeRatio = (value) => {
    const v = String(value || '').trim()
    // Flow 没有通用 Auto 选项，API 传 auto 或未知比例时按产品约定退到 9:16。
    return ['16:9', '4:3', '1:1', '3:4', '9:16'].includes(v) ? v : '9:16'
  }
  const targetRatio = normalizeRatio(params.ratio)
  const targetCount = params.count || 1

  const ratioMap = {
    '16:9': 'LANDSCAPE',
    '4:3': 'LANDSCAPE_4_3',
    '1:1': 'SQUARE',
    '3:4': 'PORTRAIT_3_4',
    '9:16': 'PORTRAIT',
  }

  // 获取元素中心坐标，供外部 CDP 点击使用
  const getCenter = (el) => {
    const rect = el.getBoundingClientRect()
    return { x: rect.x + rect.width / 2, y: rect.y + rect.height / 2 }
  }

  // 真实鼠标事件点击（通过 MouseEvent，比 PointerEvent 更接近真实用户）
  const clickAt = (el) => {
    const { x, y } = getCenter(el)
    el.scrollIntoView({ block: 'center' })
    for (const type of ['pointerdown', 'mousedown', 'pointerup', 'mouseup', 'click']) {
      el.dispatchEvent(
        new PointerEvent(type, {
          bubbles: true,
          cancelable: true,
          composed: true,
          clientX: x,
          clientY: y,
          button: 0,
          pointerId: 1,
          pointerType: 'mouse',
          view: window,
        })
      )
    }
  }

  const findSelectorBtn = () => {
    for (const btn of document.querySelectorAll('button[aria-haspopup="menu"]')) {
      const text = btn.textContent || ''
      if (text.includes('crop_') || text.includes('Banana') || text.includes('Imagen')) return btn
    }
    return null
  }

  const openMenuAndWait = async () => {
    const btn = findSelectorBtn()
    if (!btn) return null
    clickAt(btn)
    for (let i = 0; i < 20; i++) {
      await sleep(200)
      const menu = document.querySelector('[role="menu"]')
      if (menu && menu.querySelectorAll('button[role="tab"]').length >= 5) return menu
    }
    return document.querySelector('[role="menu"]')
  }

  try {
    const results = { ratio: false, count: false, model: false }

    const menu = await openMenuAndWait()
    if (!menu) return JSON.stringify({ error: '选择器菜单未弹出' })

    // 步骤 1: 选择比例
    const ratioSuffix = ratioMap[targetRatio] || 'PORTRAIT'
    for (const tab of menu.querySelectorAll('button[role="tab"]')) {
      if ((tab.id || '').endsWith(`trigger-${ratioSuffix}`)) {
        clickAt(tab)
        await sleep(500)
        results.ratio = true
        break
      }
    }

    // 步骤 2: 选择数量
    for (const tab of menu.querySelectorAll('button[role="tab"]')) {
      const id = tab.id || ''
      const text = tab.textContent?.trim() || ''
      if (id.endsWith(`trigger-${targetCount}`) && text === `x${targetCount}`) {
        clickAt(tab)
        await sleep(500)
        results.count = true
        break
      }
    }

    // 步骤 3: 选择模型（最后，二级菜单会关闭主菜单）
    const modelBtn = menu.querySelector('button[aria-haspopup="menu"]')
    if (modelBtn && !(modelBtn.textContent?.trim() || '').includes(targetModel)) {
      clickAt(modelBtn)
      await sleep(1000)
      for (const m of document.querySelectorAll('[role="menu"]')) {
        for (const item of m.querySelectorAll('[role="menuitem"]')) {
          if ((item.textContent?.trim() || '').includes(targetModel)) {
            clickAt(item)
            results.model = true
            break
          }
        }
        if (results.model) break
      }
    } else {
      results.model = true
    }

    // 关闭菜单
    await sleep(300)
    document.dispatchEvent(
      new KeyboardEvent('keydown', {
        key: 'Escape',
        code: 'Escape',
        bubbles: true,
        cancelable: true,
      })
    )
    await sleep(200)
    document.body.click()
    await sleep(300)

    return JSON.stringify({
      selected: results.model && results.ratio && results.count,
      model: targetModel,
      ratio: targetRatio,
      count: targetCount,
      details: results,
    })
  } catch (e) {
    return JSON.stringify({ error: String(e?.message || e) })
  }
})()
