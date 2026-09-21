import { useCallback, useEffect, useState } from 'react'
import type { PageId } from '../../App'

export const pageIds: PageId[] = [
  'accounts',
  'api-keys',
  'api-docs',
  'tasks',
  'generation-queue',
  'weixin',
  'proxies',
  'verifications',
  'logs',
  'settings',
  'admin-users',
]
export function readPageRoute() {
  const path = window.location.pathname.replace(/\/+$/, '') || '/accounts'
  const page = pageIds.find((id) => path === '/' + id) || null
  const taskId = new URLSearchParams(window.location.search).get('task_id')
  return { page, taskId }
}
export function usePageRoute() {
  const [route, setRoute] = useState(readPageRoute)
  useEffect(() => {
    if (window.location.pathname === '/')
      window.history.replaceState(null, '', '/accounts' + window.location.search)
    const pop = () => setRoute(readPageRoute())
    window.addEventListener('popstate', pop)
    return () => window.removeEventListener('popstate', pop)
  }, [])
  const navigate = useCallback((page: PageId, taskId?: string) => {
    const query = taskId ? '?' + new URLSearchParams({ task_id: taskId }).toString() : ''
    const target = '/' + page + query
    if (window.location.pathname + window.location.search !== target)
      window.history.pushState(null, '', target)
    setRoute(readPageRoute())
  }, [])
  return { route, navigate }
}
