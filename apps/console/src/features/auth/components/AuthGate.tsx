import { type FormEvent, type ReactNode, useEffect, useState } from 'react'
import { LockKeyhole } from 'lucide-react'
import { getAuthStatus, loginAdmin, type AuthStatus } from '../../../shared/services/auth-service'
import { KtButton, KtInput } from '../../../shared/ui'

interface AuthGateProps {
  children: ReactNode
}

type AuthViewState = 'loading' | 'login' | 'ready'

export default function AuthGate({ children }: AuthGateProps) {
  const [viewState, setViewState] = useState<AuthViewState>('loading')

  useEffect(() => {
    let cancelled = false
    getAuthStatus()
      .then((status) => {
        if (cancelled) return
        setViewState(status.authenticated || !status.configured ? 'ready' : 'login')
      })
      .catch(() => {
        if (!cancelled) setViewState('login')
      })
    return () => {
      cancelled = true
    }
  }, [])

  if (viewState === 'loading') return <AuthLoading />
  if (viewState === 'login') return <LoginScreen onLogin={() => setViewState('ready')} />
  return <>{children}</>
}

function AuthLoading() {
  return (
    <div className="auth-screen">
      <div className="auth-panel">
        <div className="auth-panel__icon">
          <LockKeyhole size={30} />
        </div>
        <h1>正在连接 MediaGen</h1>
        <p>正在确认管理台登录状态。</p>
      </div>
    </div>
  )
}

function LoginScreen({ onLogin }: { onLogin: (status: AuthStatus) => void }) {
  const [username, setUsername] = useState('admin')
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')
  const [submitting, setSubmitting] = useState(false)

  const submit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    setError('')
    setSubmitting(true)
    try {
      onLogin(await loginAdmin({ username, password }))
    } catch (err) {
      setError(err instanceof Error ? err.message : '登录失败，请检查账号密码')
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div className="auth-screen">
      <form className="auth-panel auth-panel--form" onSubmit={submit}>
        <div className="auth-panel__icon">
          <LockKeyhole size={30} />
        </div>
        <h1>登录 MediaGen</h1>
        <p>服务端管理台已开启登录保护，请先登录后再添加账号和 API Key。</p>
        <label className="auth-field">
          <span>账号</span>
          <KtInput
            autoFocus
            value={username}
            onChange={(event) => setUsername(event.target.value)}
            placeholder="请输入管理员账号"
          />
        </label>
        <label className="auth-field">
          <span>密码</span>
          <KtInput
            type="password"
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            placeholder="请输入管理员密码"
          />
        </label>
        {error && <div className="auth-error">{error}</div>}
        <KtButton
          type="submit"
          variant="primary"
          size="lg"
          loading={submitting}
          disabled={!username.trim() || !password}
          style={{ width: '100%' }}
        >
          登录管理台
        </KtButton>
      </form>
    </div>
  )
}
