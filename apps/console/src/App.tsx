import { usePageRoute } from './app/routing/usePageRoute'
import AppShell from './app/layout/AppShell'
import AuthGate from './features/auth/components/AuthGate'

export type PageId =
  | 'accounts'
  | 'api-keys'
  | 'tasks'
  | 'generation-queue'
  | 'weixin'
  | 'proxies'
  | 'verifications'
  | 'logs'
  | 'settings'
  | 'api-docs'
  | 'admin-users'

export default function App() {
  const { route, navigate } = usePageRoute()
  return (
    <AuthGate>
      {(auth) =>
        route.page ? (
          <AppShell
            adminEnabled={auth.supported}
            currentPage={route.page}
            verificationTaskId={route.page === 'verifications' ? route.taskId : null}
            logTaskId={route.page === 'logs' ? route.taskId : null}
            onNavigate={navigate}
            onOpenVerification={(taskId) => navigate('verifications', taskId)}
            onOpenTaskLogs={(taskId) => navigate('logs', taskId)}
          />
        ) : (
          <div className="auth-screen">
            <div className="auth-panel">
              <h1>页面不存在</h1>
              <p>请检查地址，或返回账号页面。</p>
              <a href="/accounts">返回账号</a>
            </div>
          </div>
        )
      }
    </AuthGate>
  )
}
