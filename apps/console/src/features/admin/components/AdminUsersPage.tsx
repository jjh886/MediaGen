import { useCallback, useEffect, useState } from 'react'
import { KtButton, KtInput, KtModal, KtConfirmDialog, KtBadge } from '@/shared/ui'
import { getAuthStatus } from '@/shared/services/auth-service'
import {
  fetchAdminUsers,
  createAdminUser,
  updateAdminUser,
  deleteAdminUser,
  resetAdminPassword,
  type AdminUser,
} from '@/shared/services/admin-users-service'

type Editor = { mode: 'create' | 'edit' | 'password'; user?: AdminUser }
export default function AdminUsersPage() {
  const [users, setUsers] = useState<AdminUser[]>([])
  const [current, setCurrent] = useState<string>()
  const [editor, setEditor] = useState<Editor | null>(null)
  const [deleting, setDeleting] = useState<AdminUser | null>(null)
  const [error, setError] = useState('')
  const [message, setMessage] = useState('')
  const [loading, setLoading] = useState(false)
  const refresh = useCallback(async () => {
    setLoading(true)
    setError('')
    try {
      const [list, status] = await Promise.all([fetchAdminUsers(), getAuthStatus()])
      setUsers(list)
      setCurrent(status.user_id)
    } catch (error) {
      setError(error instanceof Error ? error.message : '用户加载失败')
    } finally {
      setLoading(false)
    }
  }, [])
  useEffect(() => {
    void refresh()
  }, [refresh])
  const remove = async () => {
    if (!deleting) return
    try {
      await deleteAdminUser(deleting.id)
      setDeleting(null)
      setMessage('用户已删除，原会话已失效')
      await refresh()
    } catch (error) {
      setError(error instanceof Error ? error.message : '删除失败')
      setDeleting(null)
    }
  }
  return (
    <div style={{ display: 'grid', gap: 20 }}>
      <header
        style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: 16 }}
      >
        <div>
          <h1 style={{ margin: 0 }}>后台用户</h1>
          <p style={{ color: 'var(--text-secondary)' }}>
            管理可登录此后台的管理员。密码重置或停用后，原登录会话立即失效。
          </p>
        </div>
        <div style={{ display: 'flex', gap: 8 }}>
          <KtButton onClick={() => void refresh()} loading={loading}>
            刷新
          </KtButton>
          <KtButton variant="primary" onClick={() => setEditor({ mode: 'create' })}>
            新增用户
          </KtButton>
        </div>
      </header>
      {error && (
        <div role="alert" style={{ color: 'var(--color-error)' }}>
          {error}
        </div>
      )}
      {message && <div role="status">{message}</div>}
      <div style={{ overflowX: 'auto', border: '1px solid var(--border-muted)', borderRadius: 12 }}>
        <table
          style={{ width: '100%', borderCollapse: 'collapse', textAlign: 'left', fontSize: 13 }}
        >
          <thead>
            <tr>
              {['用户名 / 名称', '状态', '最近登录', '创建时间', '操作'].map((label) => (
                <th key={label} style={{ padding: 16, background: 'var(--bg-secondary)' }}>
                  {label}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {users.map((user) => (
              <tr key={user.id}>
                <td style={{ padding: 16 }}>
                  <strong>{user.username}</strong>
                  {user.id === current && <small> · 当前用户</small>}
                  <div style={{ color: 'var(--text-secondary)', marginTop: 6 }}>
                    {user.display_name || '未设置显示名称'}
                  </div>
                </td>
                <td>
                  <KtBadge variant={user.enabled ? 'success' : 'disabled'}>
                    {user.enabled ? '启用' : '停用'}
                  </KtBadge>
                </td>
                <td>
                  {user.last_login_at ? new Date(user.last_login_at).toLocaleString() : '从未登录'}
                </td>
                <td>{new Date(user.created_at).toLocaleString()}</td>
                <td>
                  <div style={{ display: 'flex', gap: 8, padding: 12 }}>
                    <KtButton
                      size="sm"
                      aria-label={`编辑 ${user.username}`}
                      onClick={() => setEditor({ mode: 'edit', user })}
                    >
                      编辑
                    </KtButton>
                    <KtButton
                      size="sm"
                      aria-label={`重置密码 ${user.username}`}
                      onClick={() => setEditor({ mode: 'password', user })}
                    >
                      重置密码
                    </KtButton>
                    <KtButton
                      size="sm"
                      aria-label={`删除 ${user.username}`}
                      disabled={
                        user.id === current ||
                        (user.enabled && users.filter((item) => item.enabled).length === 1)
                      }
                      onClick={() => setDeleting(user)}
                    >
                      删除
                    </KtButton>
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {!users.length && !loading && (
          <p style={{ padding: 24 }}>
            还没有后台用户，点击“新增用户”创建。Linux 部署始终需要登录。
          </p>
        )}
      </div>
      {editor && (
        <UserEditor
          editor={editor}
          current={current}
          onClose={() => setEditor(null)}
          onSaved={async (id, sessionChanged) => {
            setEditor(null)
            setMessage('已保存，密码不会在列表中显示')
            if (id === current && sessionChanged) {
              window.location.reload()
              return
            }
            await refresh()
          }}
        />
      )}
      <KtConfirmDialog
        open={!!deleting}
        title="删除后台用户"
        message={`确认删除「${deleting?.username || ''}」？其登录会话会立即失效。`}
        confirmText="删除"
        danger
        onCancel={() => setDeleting(null)}
        onConfirm={remove}
      />
    </div>
  )
}
function UserEditor({
  editor,
  current,
  onClose,
  onSaved,
}: {
  editor: Editor
  current?: string
  onClose: () => void
  onSaved: (id: string, sessionChanged: boolean) => Promise<void>
}) {
  const [username, setUsername] = useState(editor.user?.username || '')
  const [display, setDisplay] = useState(editor.user?.display_name || '')
  const [enabled, setEnabled] = useState(editor.user?.enabled ?? true)
  const [password, setPassword] = useState('')
  const [confirm, setConfirm] = useState('')
  const [error, setError] = useState('')
  const [saving, setSaving] = useState(false)
  const needsPassword = editor.mode !== 'edit'
  const submit = async () => {
    setError('')
    if (needsPassword && (password.length < 12 || password !== confirm)) {
      setError(password !== confirm ? '两次输入的密码不一致' : '密码至少 12 个字符')
      return
    }
    setSaving(true)
    try {
      let id = editor.user?.id || ''
      if (editor.mode === 'create') {
        id = (
          await createAdminUser({
            username: username.trim(),
            display_name: display.trim(),
            password,
          })
        ).id
      } else if (editor.mode === 'password') {
        await resetAdminPassword(id, password)
      } else
        await updateAdminUser(id, {
          username: username.trim(),
          display_name: display.trim(),
          enabled,
        })
      await onSaved(id, editor.mode === 'password' || username !== editor.user?.username)
    } catch (error) {
      setError(error instanceof Error ? error.message : '保存失败')
    } finally {
      setSaving(false)
    }
  }
  return (
    <KtModal
      open
      onClose={onClose}
      title={
        editor.mode === 'create'
          ? '新增后台用户'
          : editor.mode === 'password'
            ? `重置密码 · ${editor.user?.username}`
            : '编辑后台用户'
      }
      width={520}
      closeOnOverlayClick={false}
      footer={
        <>
          <KtButton onClick={onClose} disabled={saving}>
            取消
          </KtButton>
          <KtButton variant="primary" loading={saving} onClick={() => void submit()}>
            保存
          </KtButton>
        </>
      }
    >
      <form
        onSubmit={(event) => {
          event.preventDefault()
          void submit()
        }}
        style={{ display: 'grid', gap: 16 }}
      >
        {editor.mode !== 'password' && (
          <>
            <label>
              用户名
              <KtInput
                aria-label="用户名"
                value={username}
                onChange={(event) => setUsername(event.target.value)}
                autoComplete="off"
                placeholder="3–64 位字母、数字、点、下划线或短横线"
              />
            </label>
            <label>
              显示名称
              <KtInput
                aria-label="显示名称"
                value={display}
                onChange={(event) => setDisplay(event.target.value)}
                maxLength={100}
              />
            </label>
          </>
        )}
        {editor.mode === 'edit' && (
          <label>
            <input
              type="checkbox"
              checked={enabled}
              disabled={editor.user?.id === current}
              onChange={(event) => setEnabled(event.target.checked)}
            />{' '}
            启用此用户
          </label>
        )}
        {needsPassword && (
          <>
            <label>
              新密码
              <KtInput
                aria-label="新密码"
                type="password"
                autoComplete="new-password"
                value={password}
                onChange={(event) => setPassword(event.target.value)}
              />
            </label>
            <label>
              确认密码
              <KtInput
                aria-label="确认密码"
                type="password"
                autoComplete="new-password"
                value={confirm}
                onChange={(event) => setConfirm(event.target.value)}
              />
            </label>
            <small style={{ color: 'var(--text-secondary)' }}>
              至少 12 个字符。重置后，该用户需使用新密码重新登录。
            </small>
          </>
        )}
        {error && (
          <div role="alert" style={{ color: 'var(--color-error)' }}>
            {error}
          </div>
        )}
      </form>
    </KtModal>
  )
}
