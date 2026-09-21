import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react'
import { Plus, RefreshCw } from 'lucide-react'
import { KtButton } from '@/shared/ui'
import { appendUnique, LIST_PAGE_SIZE } from '@/shared/services/list-pagination'
import {
  createApiKey,
  createProxy,
  updateProxy,
  deleteApiKey,
  deleteProxy,
  fetchApiKeys,
  fetchProxies,
  testProxy,
  type ApiKeyItem,
  type ProxyItem,
} from '../services/admin-service'
import {
  ApiKeyCreateDialog,
  ProxyCreateDialog,
  type ApiKeyCreateValues,
  type ProxyCreateValues,
} from './AdminCreateDialogs'
import { AdminEntityList } from './AdminEntityList'
export { default as VerificationSessionsPage } from './VerificationSessionsPage'

const pageStyle: React.CSSProperties = { display: 'flex', flexDirection: 'column', gap: 16 }

function PageTitle({
  title,
  onRefresh,
  action,
}: {
  title: string
  onRefresh: () => void
  action?: ReactNode
}) {
  return (
    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
      <h1 style={{ font: 'var(--font-h1)', color: 'var(--text-bright)' }}>{title}</h1>
      <div style={{ display: 'flex', gap: 8 }}>
        {action}
        <KtButton onClick={onRefresh}>
          <RefreshCw size={16} /> 刷新
        </KtButton>
      </div>
    </div>
  )
}

function useListData<T extends { id: string }>(
  loader: (limit?: number, offset?: number) => Promise<T[]>
) {
  const [items, setItems] = useState<T[]>([])
  const [loading, setLoading] = useState(true)
  const [loadingMore, setLoadingMore] = useState(false)
  const [hasMore, setHasMore] = useState(true)
  const [nextOffset, setNextOffset] = useState(0)
  const [error, setError] = useState('')
  const load = useCallback(async () => {
    setLoading(true)
    setError('')
    try {
      const page = await loader(LIST_PAGE_SIZE, 0)
      setItems(page)
      setHasMore(page.length === LIST_PAGE_SIZE)
      setNextOffset(page.length)
    } catch (err) {
      setError(err instanceof Error ? err.message : '加载失败，请重试。')
    } finally {
      setLoading(false)
    }
  }, [loader])
  const loadMore = useCallback(async () => {
    if (!hasMore || loadingMore) return
    setLoadingMore(true)
    try {
      const page = await loader(LIST_PAGE_SIZE, nextOffset)
      setItems((current) => appendUnique(current, page, (item) => item.id))
      setHasMore(page.length === LIST_PAGE_SIZE)
      setNextOffset((current) => current + page.length)
    } catch (err) {
      setError(err instanceof Error ? err.message : '加载失败，请重试。')
    } finally {
      setLoadingMore(false)
    }
  }, [hasMore, loader, loadingMore, nextOffset])
  useEffect(() => void load(), [load])
  return { items, loading, loadingMore, hasMore, error, load, loadMore }
}

export function ApiKeysPage() {
  const apiKeys = useListData(fetchApiKeys)
  const pageRef = useRef<HTMLDivElement>(null)
  const [dialogOpen, setDialogOpen] = useState(false)
  const refresh = useCallback(async () => {
    pageRef.current?.closest('.app-shell__main')?.scrollTo({ top: 0 })
    await apiKeys.load()
  }, [apiKeys.load])
  const submit = async (values: ApiKeyCreateValues) => {
    const result = await createApiKey(values.name)
    void refresh()
    return result
  }
  return (
    <div ref={pageRef} style={pageStyle}>
      <PageTitle
        title="API Key"
        onRefresh={() => void refresh()}
        action={
          <KtButton variant="primary" onClick={() => setDialogOpen(true)}>
            <Plus size={16} /> 新增 API Key
          </KtButton>
        }
      />
      <ApiKeyCreateDialog
        open={dialogOpen}
        onClose={() => setDialogOpen(false)}
        onSubmit={submit}
      />
      <ApiKeyList
        items={apiKeys.items}
        loading={apiKeys.loading}
        error={apiKeys.error}
        onRetry={apiKeys.load}
        hasMore={apiKeys.hasMore}
        loadingMore={apiKeys.loadingMore}
        onLoadMore={() => void apiKeys.loadMore()}
        onDelete={(item) => deleteApiKey(item.id).then(refresh)}
      />
    </div>
  )
}

function ApiKeyList(props: {
  items: ApiKeyItem[]
  loading: boolean
  error: string
  onRetry: () => void
  hasMore: boolean
  loadingMore: boolean
  onLoadMore: () => void
  onDelete: (item: ApiKeyItem) => Promise<void>
}) {
  return (
    <AdminEntityList
      {...props}
      emptyText="还没有 API Key。新增后，外部客户端才能调用 OpenAI 兼容接口。"
      columns={(item) => [item.name, item.masked_key, item.created_at]}
      deleteTitle="删除 API Key"
      deleteMessage={(item) => `确定删除「${item.name}」吗？删除后使用它的外部请求会立刻失败。`}
    />
  )
}

export function ProxiesPage() {
  const proxies = useListData(fetchProxies)
  const pageRef = useRef<HTMLDivElement>(null)
  const proxyTests = useProxyTests()
  const [dialogOpen, setDialogOpen] = useState(false)
  const [editing, setEditing] = useState<ProxyItem | null>(null)
  const refresh = useCallback(async () => {
    pageRef.current?.closest('.app-shell__main')?.scrollTo({ top: 0 })
    await proxies.load()
  }, [proxies.load])
  const submit = async (values: ProxyCreateValues) => {
    if (editing)
      await updateProxy(editing.id, { ...values, password: values.password || undefined })
    else await createProxy(values)
    await refresh()
  }
  return (
    <div ref={pageRef} style={pageStyle}>
      <PageTitle
        title="代理"
        onRefresh={refresh}
        action={
          <KtButton
            variant="primary"
            onClick={() => {
              setEditing(null)
              setDialogOpen(true)
            }}
          >
            <Plus size={16} /> 新增代理
          </KtButton>
        }
      />
      <ProxyCreateDialog
        open={dialogOpen}
        initial={editing}
        onClose={() => setDialogOpen(false)}
        onSubmit={submit}
      />
      <AdminEntityList
        items={proxies.items}
        loading={proxies.loading}
        error={proxies.error}
        onRetry={proxies.load}
        hasMore={proxies.hasMore}
        loadingMore={proxies.loadingMore}
        onLoadMore={() => void proxies.loadMore()}
        emptyText="还没有代理。新增后可在添加账号或账号配置中绑定使用。"
        columns={(item) => proxyColumns(item, proxyTests.tests[item.id])}
        renderActions={(item) => (
          <>
            <KtButton
              size="sm"
              aria-label={`编辑代理 ${item.name}`}
              onClick={() => {
                setEditing(item)
                setDialogOpen(true)
              }}
            >
              编辑
            </KtButton>
            <ProxyTestButton
              item={item}
              state={proxyTests.tests[item.id]}
              onTest={proxyTests.run}
            />
          </>
        )}
        onDelete={(item) => deleteProxy(item.id).then(refresh)}
        deleteTitle="删除代理"
        deleteMessage={(item) => `确定删除「${item.name}」吗？继续使用它的账号任务可能会失败。`}
      />
    </div>
  )
}

interface ProxyTestState {
  loading: boolean
  message: string
  ok?: boolean
}

function useProxyTests() {
  const [tests, setTests] = useState<Record<string, ProxyTestState>>({})
  const run = useCallback(async (proxy: ProxyItem) => {
    setTests((prev) => ({ ...prev, [proxy.id]: { loading: true, message: '测试中...' } }))
    try {
      const result = await testProxy(proxy.id)
      setTests((prev) => ({
        ...prev,
        [proxy.id]: { loading: false, ok: true, message: `测试通过（HTTP ${result.status}）` },
      }))
    } catch (error) {
      setTests((prev) => ({
        ...prev,
        [proxy.id]: { loading: false, ok: false, message: errorMessage(error) },
      }))
    }
  }, [])
  return { run, tests }
}

function proxyColumns(item: ProxyItem, state?: ProxyTestState) {
  return [
    item.name,
    `${item.server} · 已绑定 ${item.bound_account_count ?? 0} 个账号`,
    state?.message || proxyAuthText(item),
  ]
}

function proxyAuthText(item: ProxyItem) {
  return item.password_configured ? '已设置认证' : '未设置认证'
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : '代理测试失败，请检查配置。'
}

function ProxyTestButton({
  item,
  onTest,
  state,
}: {
  item: ProxyItem
  onTest: (item: ProxyItem) => void
  state?: ProxyTestState
}) {
  return (
    <KtButton
      aria-label={`测试代理 ${item.name}`}
      loading={state?.loading}
      size="sm"
      onClick={() => onTest(item)}
    >
      测试
    </KtButton>
  )
}
