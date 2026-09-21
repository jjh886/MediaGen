import { type FormEvent, type ReactNode, useEffect, useState } from 'react'
import { Eye, EyeOff, Layers, LoaderCircle, LockKeyhole, WifiOff } from 'lucide-react'
import './auth.css'
import { getAuthStatus, loginAdmin, type AuthStatus } from '../../../shared/services/auth-service'
import { KtButton, KtInput } from '../../../shared/ui'

interface AuthGateProps {
  children: ReactNode | ((status: AuthStatus) => ReactNode)
}

export default function AuthGate({ children }: AuthGateProps) {
  const [status, setStatus] = useState<AuthStatus | null>(null)
  const [connectionError, setConnectionError] = useState(false)
  const [attempt, setAttempt] = useState(0)
  useEffect(() => {
    let cancelled = false
    setConnectionError(false)
    getAuthStatus()
      .then((value) => {
        if (!cancelled) setStatus(value)
      })
      .catch(() => {
        if (!cancelled) setConnectionError(true)
      })
    return () => {
      cancelled = true
    }
  }, [attempt])
  useEffect(() => {
    const expired = () =>
      setStatus((current) => (current?.supported ? { ...current, authenticated: false } : current))
    window.addEventListener('mediagen:auth-expired', expired)
    return () => window.removeEventListener('mediagen:auth-expired', expired)
  }, [])
  if (connectionError)
    return (
      <AuthFrame>
        <div className="auth-panel">
          <WifiOff size={30} />
          <h1>无法连接服务</h1>
          <p>请确认 MediaGen 服务正在运行，再重试连接。</p>
          <KtButton variant="primary" onClick={() => setAttempt((value) => value + 1)}>
            重试连接
          </KtButton>
        </div>
      </AuthFrame>
    )
  if (!status)
    return (
      <AuthFrame>
        <div className="auth-panel" role="status">
          <LoaderCircle size={30} />
          <h1>正在连接 MediaGen</h1>
          <p>正在确认工作空间状态。</p>
        </div>
      </AuthFrame>
    )
  if (status.supported && !status.configured)
    return (
      <AuthFrame>
        <div className="auth-panel">
          <LockKeyhole size={30} />
          <h1>初始化管理员</h1>
          <p>
            请在服务器配置 MEDIAGEN_ADMIN_USERNAME 和 MEDIAGEN_ADMIN_PASSWORD
            后重启。首次账号创建后，可在“后台用户”中新增用户和重置密码。
          </p>
        </div>
      </AuthFrame>
    )
  if (status.supported && !status.authenticated) return <LoginScreen onLogin={setStatus} />
  return <>{typeof children === 'function' ? children(status) : children}</>
}

function AuthFrame({ children }: { children: ReactNode }) {
  return (
    <div className="auth-screen">
      <div className="auth-layout">
        <section className="auth-intro" aria-label="MediaGen 工作空间">
          <div className="auth-brand">
            <Layers size={24} />
            <strong>MediaGen</strong>
            <span>STUDIO</span>
          </div>
          <div className="auth-intro__body">
            <span className="auth-eyebrow">生成工作空间</span>
            <h2>
              创作有序，
              <br />
              一处掌控。
            </h2>
            <p>
              连接 AI 账号，管理生成任务，
              <br />
              让每一次调用都有迹可循。
            </p>
          </div>
          <div className="auth-intro__footer">
            <span>账号与代理</span>
            <span>任务与队列</span>
            <span>接口与调用</span>
          </div>
        </section>
        <div className="auth-form-side">
          {children}
          <p className="auth-footnote">MediaGen · 你的生成控制台</p>
        </div>
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
    <AuthFrame>
      <form className="auth-panel auth-panel--form" onSubmit={submit}>
        <div className="auth-eyebrow">
          <LockKeyhole size={14} /> 服务器工作空间
        </div>
        <h1>登录 MediaGen</h1>
        <p>使用管理员为你分配的账号登录。</p>
        <LoginFields
          username={username}
          password={password}
          onUsernameChange={setUsername}
          onPasswordChange={setPassword}
        />
        {error && (
          <div role="alert" className="auth-error">
            {error}
          </div>
        )}
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
        <p className="auth-password-help">忘记密码？请联系管理员重置。</p>
      </form>
    </AuthFrame>
  )
}

interface LoginFieldsProps {
  username: string
  password: string
  onUsernameChange: (value: string) => void
  onPasswordChange: (value: string) => void
}

function LoginFields({ username, password, onUsernameChange, onPasswordChange }: LoginFieldsProps) {
  const [showPassword, setShowPassword] = useState(false)
  return (
    <>
      <label className="auth-field">
        <span>账号</span>
        <KtInput
          autoFocus
          autoComplete="username"
          value={username}
          onChange={(event) => onUsernameChange(event.target.value)}
          placeholder="请输入管理员账号"
        />
      </label>
      <div className="auth-field">
        <label htmlFor="admin-password">密码</label>
        <span className="auth-password">
          <KtInput
            id="admin-password"
            type={showPassword ? 'text' : 'password'}
            autoComplete="current-password"
            value={password}
            onChange={(event) => onPasswordChange(event.target.value)}
            placeholder="请输入管理员密码"
            maxLength={1024}
          />
          <button
            type="button"
            aria-label={showPassword ? '隐藏密码' : '显示密码'}
            onClick={() => setShowPassword(!showPassword)}
          >
            {showPassword ? <EyeOff size={18} /> : <Eye size={18} />}
          </button>
        </span>
      </div>
    </>
  )
}
