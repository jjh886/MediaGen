export const LIST_PAGE_SIZE = 40

export function pagedQuery(limit = LIST_PAGE_SIZE, offset = 0): URLSearchParams {
  return new URLSearchParams({ limit: String(limit), offset: String(offset) })
}

/** 追加分页结果时按主键去重，避免列表顶部新增数据造成 offset 页面重复。 */
export function appendUnique<T>(current: T[], next: T[], keyOf: (item: T) => string): T[] {
  const seen = new Set(current.map(keyOf))
  return [...current, ...next.filter((item) => !seen.has(keyOf(item)))]
}

/** 统一分页请求的 loading 生命周期，保证成功或失败后都能恢复交互状态。 */
export async function runPageRequest<T>(
  setLoading: (loading: boolean) => void,
  request: () => Promise<T>,
  onError: (error: unknown) => void
): Promise<T> {
  setLoading(true)
  try {
    return await request()
  } catch (error) {
    onError(error)
    throw error
  } finally {
    setLoading(false)
  }
}

export function listErrorMessage(error: unknown, fallback: string): string {
  return error instanceof Error ? error.message : fallback
}
