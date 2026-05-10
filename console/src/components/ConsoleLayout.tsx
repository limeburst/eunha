import { Link, useNavigate } from 'react-router-dom'
import { Trans } from '@lingui/macro'
import { useAuthStore } from '../store/auth'

export function ConsoleLayout({ children }: { children: React.ReactNode }) {
  const { user, logout } = useAuthStore()
  const navigate = useNavigate()

  return (
    <div className="min-h-screen bg-bg text-text">
      <main className="max-w-2xl mx-auto px-6 py-8 space-y-8">
        <div className="flex items-center justify-between">
          <Link to="/dashboard" className="text-xs tracking-widest uppercase text-muted hover:text-text transition-colors">
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

        {children}
      </main>
    </div>
  )
}
