// 生成完成后删除本次即梦会话。优先删除 mark_submit.js 打过标记的会话；
// 如果提交前还没有 active 会话，则只在 active 会话缩略图匹配本次生成结果时删除。
;(async () => {
  const SK = Symbol.for('_jimeng')
  const state = window[SK] || {}
  const snapshot = state.conversation_snapshot || {}
  const markerAttr = snapshot.attr || 'data-mediagen-jimeng-task-conversation'
  const token = snapshot.token || state.task_token || ''
  const sleep = (ms) => new Promise((resolve) => window.setTimeout(resolve, ms))

  const isVisible = (el) => {
    const rect = el?.getBoundingClientRect?.()
    if (!rect || Math.min(rect.width, rect.height) <= 0) return false
    const style = window.getComputedStyle?.(el)
    return (
      !style ||
      !(style.display === 'none' || style.visibility === 'hidden' || style.opacity === '0')
    )
  }

  const textOf = (el) => (el?.textContent || '').replace(/\s+/g, ' ').trim()
  const classOf = (el) => String(el?.className || '')
  const extractAssetKey = (value) => {
    const text = String(value || '')
    const match = text.match(/(?:^|\/)([0-9a-f]{24,})(?=$|[~/?#.:_-])/i)
    return match ? match[1].toLowerCase() : ''
  }
  const cssEscape = (value) => {
    if (window.CSS?.escape) return window.CSS.escape(String(value))
    return String(value).replace(/["\\]/g, '\\$&')
  }

  const clickLikeUser = (node) => {
    if (!node) return false
    const EventCtor = typeof PointerEvent === 'function' ? PointerEvent : MouseEvent
    const rect = node.getBoundingClientRect?.()
    const clientX = rect ? rect.left + rect.width / 2 : 0
    const clientY = rect ? rect.top + rect.height / 2 : 0
    const opts = { bubbles: true, cancelable: true, view: window, clientX, clientY }
    try {
      node.dispatchEvent(new EventCtor('pointerdown', opts))
    } catch (_) {}
    try {
      node.dispatchEvent(new MouseEvent('mousedown', opts))
    } catch (_) {}
    try {
      node.dispatchEvent(new EventCtor('pointerup', opts))
    } catch (_) {}
    try {
      node.dispatchEvent(new MouseEvent('mouseup', opts))
    } catch (_) {}
    try {
      node.click?.()
    } catch (_) {}
    return true
  }

  const hoverLikeUser = (node) => {
    if (!node) return false
    try {
      node.scrollIntoView?.({ block: 'nearest', inline: 'nearest' })
    } catch (_) {}
    const EventCtor = typeof PointerEvent === 'function' ? PointerEvent : MouseEvent
    const rect = node.getBoundingClientRect?.()
    const clientX = rect ? rect.left + rect.width / 2 : 0
    const clientY = rect ? rect.top + rect.height / 2 : 0
    const opts = { bubbles: true, cancelable: true, view: window, clientX, clientY }
    for (const type of [
      'pointerover',
      'mouseover',
      'pointerenter',
      'mouseenter',
      'pointermove',
      'mousemove',
    ]) {
      try {
        const Ctor = type.startsWith('pointer') ? EventCtor : MouseEvent
        node.dispatchEvent(new Ctor(type, opts))
      } catch (_) {}
    }
    return true
  }

  const targetTitle = (shell) => {
    const named = shell?.querySelector?.('[class*="item-name"]')
    const item = shell?.querySelector?.('[class*="conversation-item"]')
    return textOf(named || item || shell)
  }

  const assetKeysIn = (root) => {
    const keys = new Set()
    if (!root) return keys

    for (const node of nodesIn(root)) {
      for (const value of assetValuesFromNode(node)) {
        addAssetKey(keys, value)
      }
    }
    return keys
  }

  const nodesIn = (root) => [root, ...(root.querySelectorAll?.('*') || [])]

  const addAssetKey = (keys, value) => {
    const key = extractAssetKey(value)
    if (key) keys.add(key)
  }

  const imageAssetValues = (node) =>
    node instanceof HTMLImageElement ? [node.currentSrc, node.src] : []

  const computedAssetValues = (node) => {
    try {
      const style = window.getComputedStyle?.(node)
      return [
        style?.backgroundImage || '',
        style?.getPropertyValue?.('--model-select-option-svg-url') || '',
      ]
    } catch (_) {
      return []
    }
  }

  const assetValuesFromNode = (node) => [
    ...imageAssetValues(node),
    ...[...(node.attributes || [])].map((attr) => attr.value),
    node.getAttribute?.('style') || '',
    ...computedAssetValues(node),
  ]

  const targetAsset = (shell) => [...assetKeysIn(shell)][0] || ''

  const resultAssetKeys = () => {
    const keys = new Set()
    for (const image of Array.isArray(state.images) ? state.images : []) {
      const fromUri = extractAssetKey(image?.image_uri || '')
      const fromUrl = extractAssetKey(image?.url || '')
      if (fromUri) keys.add(fromUri)
      if (fromUrl) keys.add(fromUrl)
    }
    return keys
  }

  const findMarkedShell = () => {
    if (!token) return null
    return document.querySelector(`[${markerAttr}="${cssEscape(token)}"]`)
  }

  const findActiveConversationShell = () => {
    const activeItems = [...document.querySelectorAll('[class*="conversation-item"]')]
      .filter((node) => /active/.test(classOf(node)))
      .filter((node) => {
        const title = targetTitle(node)
        return title && !/默认创作/.test(title)
      })

    const shells = []
    for (const item of activeItems) {
      const shell = item.closest?.('[class*="tooltip-trigger-shell"]') || item
      if (shell && !shells.includes(shell)) shells.push(shell)
    }

    if (shells.length !== 1) {
      return {
        shell: null,
        reason:
          shells.length === 0 ? '未找到 active 会话' : `active 会话数量异常: ${shells.length}`,
      }
    }
    return { shell: shells[0], reason: '' }
  }

  const identityStillLooksSafe = (shell) => {
    if (!shell || !snapshot.marked) return false
    const title = targetTitle(shell)
    if (!title || /默认创作/.test(title)) return false
    return true
  }

  const findDeletionTarget = () => {
    const marked = findMarkedShell()
    if (identityStillLooksSafe(marked)) {
      return {
        shell: marked,
        source: 'marker',
        title: targetTitle(marked),
        assetKey: targetAsset(marked),
      }
    }

    const keys = resultAssetKeys()
    if (keys.size === 0) {
      return {
        shell: null,
        reason: snapshot.reason || '没有可校验的本次结果图片 key',
        resultKeys: [],
        waitable: false,
      }
    }

    const active = findActiveConversationShell()
    if (!active.shell) {
      return {
        shell: null,
        reason: active.reason,
        resultKeys: [...keys],
        waitable: true,
      }
    }

    const activeKeys = assetKeysIn(active.shell)
    const matchedKey = [...activeKeys].find((key) => keys.has(key)) || ''
    const activeAsset = matchedKey || [...activeKeys][0] || ''
    const activeTitle = targetTitle(active.shell)
    if (!matchedKey) {
      return {
        shell: null,
        reason: 'active 会话缩略图不属于本次生成结果，跳过删除避免误删',
        activeTitle,
        activeAsset,
        activeKeys: [...activeKeys],
        resultKeys: [...keys],
        waitable: activeKeys.size === 0,
      }
    }

    return {
      shell: active.shell,
      source: 'active_result_thumbnail',
      title: activeTitle,
      assetKey: matchedKey,
      resultKeys: [...keys],
    }
  }

  const waitForDeletionTarget = async () => {
    let last = null
    const deadline = Date.now() + 10000
    while (Date.now() < deadline) {
      const target = findDeletionTarget()
      if (target.shell) return target
      if (!target.resultKeys || target.resultKeys.length === 0) return target
      if (!target.waitable) return target
      last = target
      await sleep(250)
    }
    return last || findDeletionTarget()
  }

  const waitFor = async (getter, timeoutMs, intervalMs = 120) => {
    const deadline = Date.now() + timeoutMs
    while (Date.now() < deadline) {
      const value = getter()
      if (value) return value
      await sleep(intervalMs)
    }
    return null
  }

  const findDeleteMenuItem = () => {
    const roots = [
      ...document.querySelectorAll(
        '[role="tooltip"], [class*="popover-content"], [class*="lv-popover"]'
      ),
    ].filter(isVisible)
    for (const root of roots) {
      const item = [
        ...root.querySelectorAll(
          '[class*="menu-item"], button, [role="menuitem"], [role="button"]'
        ),
      ].find((node) => isVisible(node) && textOf(node) === '删除')
      if (item) return item
    }
    return null
  }

  const clickConfirmDeleteIfAny = async () => {
    const button = await waitFor(() => {
      const roots = [
        ...document.querySelectorAll(
          '[role="dialog"], [aria-modal="true"], [class*="modal"], [class*="dialog"]'
        ),
      ]
        .filter(isVisible)
        .filter((root) => /删除|确认/.test(textOf(root)))
      for (const root of roots) {
        const candidate = [...root.querySelectorAll('button, [role="button"]')]
          .filter(isVisible)
          .find((node) => /^(删除|确认|确定)$/.test(textOf(node)) && !/取消/.test(textOf(node)))
        if (candidate) return candidate
      }
      return null
    }, 2500)
    if (!button) return false
    clickLikeUser(button)
    return true
  }

  try {
    const target = await waitForDeletionTarget()
    const shell = target.shell
    if (!shell) {
      return JSON.stringify({
        deleted: false,
        skipped: true,
        reason: target.reason || '未找到可安全删除的本次会话',
        snapshotTitle: snapshot.title || '',
        currentTitle: target.activeTitle || '',
        currentAsset: target.activeAsset || '',
        currentAssets: target.activeKeys || [],
        resultKeys: target.resultKeys || [],
      })
    }

    const item = shell.querySelector?.('[class*="conversation-item"]') || shell
    hoverLikeUser(item)
    hoverLikeUser(shell)
    const more = await waitFor(() => {
      const candidates = [
        ...(item.querySelectorAll?.('[class*="more-button"]') || []),
        ...(shell.querySelectorAll?.('[class*="more-button"]') || []),
      ]
      return candidates.find(isVisible) || candidates[0] || null
    }, 3000)
    if (!more) {
      return JSON.stringify({ deleted: false, skipped: true, reason: '标记会话没有更多按钮' })
    }

    hoverLikeUser(more)
    clickLikeUser(more)
    const deleteItem = await waitFor(findDeleteMenuItem, 3000)
    if (!deleteItem) {
      return JSON.stringify({
        deleted: false,
        skipped: true,
        reason: '未找到删除菜单项',
        title: target.title || snapshot.title || '',
        source: target.source || '',
      })
    }

    clickLikeUser(deleteItem)
    const confirmed = await clickConfirmDeleteIfAny()

    const removed = await waitFor(
      () => !shell.isConnected || (target.source === 'marker' && !findMarkedShell()),
      5000,
      200
    )
    if (removed) {
      return JSON.stringify({
        deleted: true,
        title: target.title || snapshot.title || '',
        assetKey: target.assetKey || '',
        source: target.source || '',
        confirmed,
      })
    }
    return JSON.stringify({
      deleted: false,
      skipped: true,
      reason: '点击删除后会话仍存在',
      title: target.title || snapshot.title || '',
      source: target.source || '',
      confirmed,
    })
  } catch (e) {
    return JSON.stringify({
      deleted: false,
      skipped: true,
      reason: `删除会话脚本异常: ${String(e?.message || e)}`,
    })
  }
})()
