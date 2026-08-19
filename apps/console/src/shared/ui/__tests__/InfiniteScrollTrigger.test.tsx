import { render } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { InfiniteScrollTrigger } from '../InfiniteScrollTrigger'

class VisibleIntersectionObserver {
  constructor(private readonly callback: IntersectionObserverCallback) {}

  observe(target: Element) {
    this.callback([{ isIntersecting: true, target } as IntersectionObserverEntry], this as never)
  }

  disconnect() {}
  unobserve() {}
  takeRecords() {
    return []
  }
  readonly root = null
  readonly rootMargin = '240px 0px'
  readonly thresholds = [0]
}

describe('InfiniteScrollTrigger', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('进入底部预加载区域时请求下一页', () => {
    vi.stubGlobal('IntersectionObserver', VisibleIntersectionObserver)
    const loadMore = vi.fn()

    render(<InfiniteScrollTrigger hasMore loading={false} onLoadMore={loadMore} />)

    expect(loadMore).toHaveBeenCalledTimes(1)
  })

  it('加载中或没有下一页时不会重复请求', () => {
    const observer = vi.fn()
    vi.stubGlobal('IntersectionObserver', observer)

    render(<InfiniteScrollTrigger hasMore loading onLoadMore={vi.fn()} />)
    render(<InfiniteScrollTrigger hasMore={false} loading={false} onLoadMore={vi.fn()} />)

    expect(observer).not.toHaveBeenCalled()
  })
})
