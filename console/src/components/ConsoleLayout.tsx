import { Link, useNavigate } from 'react-router-dom'
import { Trans } from '@lingui/macro'
import { useAuthStore } from '../store/auth'

export function ConsoleLayout({ children }: { children: React.ReactNode }) {
  const { user, logout } = useAuthStore()
  const navigate = useNavigate()

  return (
    <div className="min-h-screen bg-bg text-text">
      <header className="border-b border-border">
        <div className="max-w-2xl mx-auto px-6 h-12 flex items-center justify-between">
          <Link to="/dashboard" className="text-sm font-medium tracking-wide text-text hover:text-muted transition-colors">
            eunha.social
          </Link>
          {user && (
            <button
              onClick={() => { logout(); navigate('/login') }}
              className="text-xs text-muted hover:text-text transition-colors"
            >
              <Trans>Sign out</Trans>
            </button>
          )}
        </div>
      </header>
      <main className="max-w-2xl mx-auto px-6 py-8 space-y-8">
        {children}
      </main>
    </div>
  )
}
