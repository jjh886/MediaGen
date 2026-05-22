// Flow: 清理项目中本次生成的结果和参考图（避免累积）
// 策略：删除项目中所有可见的图片项（右键 → "删除"），直到项目为空或回到初始状态
// 比按 name 匹配更可靠，因为新生成的图片可能不在 DOM 中或 URL 格式不同
;(async () => {
  function sleep(ms) {
    return new Promise((r) => setTimeout(r, ms))
  }

  try {
    // 等待新图片出现在 DOM 中（刚生成的可能还没渲染）
    await sleep(2000)

    // 找到所有可删除的项目（div[role=button] 包含 img）
    const findItems = () => {
      const items = []
      const allItems = document.querySelectorAll(
        'div[role="button"][tabindex="0"][aria-disabled="false"]'
      )
      for (const item of allItems) {
        const img = item.querySelector('img[src*="getMediaUrlRedirect"]')
        if (img) items.push(item)
      }
      return items
    }

    // 删除单个项目：右键 → 点击"删除"
    const deleteItem = async (item) => {
      // 滚动到可见区域
      item.scrollIntoView({ block: 'center' })
      await sleep(300)

      // 右键点击触发上下文菜单
      const rect = item.getBoundingClientRect()
      const x = rect.x + rect.width / 2
      const y = rect.y + rect.height / 2

      item.dispatchEvent(
        new PointerEvent('contextmenu', {
          bubbles: true,
          cancelable: true,
          composed: true,
          clientX: x,
          clientY: y,
          button: 2,
        })
      )
      await sleep(800)

      // 找到"删除"菜单项
      const menuItems = document.querySelectorAll('[role="menuitem"]')
      for (const mi of menuItems) {
        const text = mi.textContent?.trim() || ''
        if (text.includes('删除') || text.includes('delete')) {
          const miRect = mi.getBoundingClientRect()
          for (const type of ['pointerdown', 'mousedown', 'pointerup', 'mouseup', 'click']) {
            mi.dispatchEvent(
              new PointerEvent(type, {
                bubbles: true,
                cancelable: true,
                composed: true,
                clientX: miRect.x + miRect.width / 2,
                clientY: miRect.y + miRect.height / 2,
                button: 0,
                pointerId: 1,
                pointerType: 'mouse',
              })
            )
          }
          await sleep(1500)
          return true
        }
      }

      // 菜单没找到删除项，关闭菜单
      document.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'Escape', code: 'Escape', bubbles: true })
      )
      await sleep(300)
      return false
    }

    // 循环删除所有项目（最多删 20 个，防止死循环）
    let deletedCount = 0
    for (let round = 0; round < 20; round++) {
      const items = findItems()
      if (items.length === 0) break

      // 删除最后一个（最新的）
      const success = await deleteItem(items[items.length - 1])
      if (success) {
        deletedCount++
      } else {
        break // 删除失败则停止
      }
    }

    // 清理状态
    window.__frame64_uploaded_refs = []

    return JSON.stringify({
      cleaned: deletedCount,
      method: 'ui_contextmenu_all',
    })
  } catch (e) {
    return JSON.stringify({ error: String(e?.message || e) })
  }
})()
