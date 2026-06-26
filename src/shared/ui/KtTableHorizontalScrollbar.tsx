import type React from 'react'
import { useEffect, useRef, useState } from 'react'

interface KtTableHorizontalScrollbarProps {
  scrollRef: React.RefObject<HTMLDivElement | null>
}

interface ScrollMetrics {
  clientWidth: number
  scrollLeft: number
  scrollWidth: number
}

const MAX_THUMB_RATIO = 0.3
const MIN_THUMB_RATIO = 0.12

const styles = {
  track: {
    height: 10,
    marginTop: 4,
    position: 'relative' as const,
    cursor: 'pointer',
  },
  thumb: {
    position: 'absolute' as const,
    top: 3,
    height: 4,
    borderRadius: 999,
    background: 'rgba(255,255,255,0.24)',
    cursor: 'grab',
  },
} as const

function readMetrics(node: HTMLDivElement | null): ScrollMetrics {
  return {
    clientWidth: node?.clientWidth ?? 0,
    scrollLeft: node?.scrollLeft ?? 0,
    scrollWidth: node?.scrollWidth ?? 0,
  }
}

function clampScrollLeft(scrollLeft: number, maxScrollLeft: number): number {
  return Math.min(Math.max(scrollLeft, 0), maxScrollLeft)
}

function getThumbRatio(metrics: ScrollMetrics): number {
  if (metrics.scrollWidth <= 0) return MAX_THUMB_RATIO
  const nativeRatio = metrics.clientWidth / metrics.scrollWidth
  return Math.min(Math.max(nativeRatio, MIN_THUMB_RATIO), MAX_THUMB_RATIO)
}

function getThumbLeft(metrics: ScrollMetrics, thumbRatio: number): string {
  const maxScrollLeft = metrics.scrollWidth - metrics.clientWidth
  if (maxScrollLeft <= 0) return '0%'

  const maxThumbLeft = 1 - thumbRatio
  return `${(metrics.scrollLeft / maxScrollLeft) * maxThumbLeft * 100}%`
}

export function KtTableHorizontalScrollbar({ scrollRef }: KtTableHorizontalScrollbarProps) {
  const trackRef = useRef<HTMLDivElement>(null)
  const [metrics, setMetrics] = useState<ScrollMetrics>(() => readMetrics(scrollRef.current))
  const hasOverflow = metrics.scrollWidth > metrics.clientWidth + 1
  const thumbRatio = getThumbRatio(metrics)
  const thumbWidth = `${thumbRatio * 100}%`

  useEffect(() => {
    const node = scrollRef.current
    if (!node) return

    const syncMetrics = () => setMetrics(readMetrics(node))
    const resizeObserver = window.ResizeObserver ? new ResizeObserver(syncMetrics) : null

    syncMetrics()
    node.addEventListener('scroll', syncMetrics)
    window.addEventListener('resize', syncMetrics)
    resizeObserver?.observe(node)

    return () => {
      node.removeEventListener('scroll', syncMetrics)
      window.removeEventListener('resize', syncMetrics)
      resizeObserver?.disconnect()
    }
  }, [scrollRef])

  if (!hasOverflow) return null

  const handlePointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    const node = scrollRef.current
    const track = trackRef.current
    if (!node || !track) return

    event.preventDefault()
    const startX = event.clientX
    const startScrollLeft = node.scrollLeft
    const maxScrollLeft = node.scrollWidth - node.clientWidth
    const maxThumbTravel = track.clientWidth * (1 - thumbRatio)

    // 自定义滑块的移动距离要换算回真实表格滚动距离，保证拖动手感自然。
    const handlePointerMove = (moveEvent: PointerEvent) => {
      const scrollDelta = ((moveEvent.clientX - startX) / maxThumbTravel) * maxScrollLeft
      node.scrollLeft = clampScrollLeft(startScrollLeft + scrollDelta, maxScrollLeft)
    }

    const handlePointerUp = () => {
      document.removeEventListener('pointermove', handlePointerMove)
      document.removeEventListener('pointerup', handlePointerUp)
    }

    document.addEventListener('pointermove', handlePointerMove)
    document.addEventListener('pointerup', handlePointerUp)
  }

  return (
    <div ref={trackRef} aria-hidden="true" style={styles.track}>
      <div
        style={{
          ...styles.thumb,
          left: getThumbLeft(metrics, thumbRatio),
          width: thumbWidth,
        }}
        onPointerDown={handlePointerDown}
      />
    </div>
  )
}
