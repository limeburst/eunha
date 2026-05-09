import { useState } from 'react'
import { Link, useNavigate } from 'react-router-dom'
import { Trans, t } from '@lingui/macro'
import { useLingui } from '@lingui/react'
import { useAuthStore } from '../store/auth'
import { useLocaleStore } from '../store/locale'
import { login } from '../api/endpoints'
import { locales, type Locale } from '../i18n'

export function Login() {
  useLingui()
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const { setAuth } = useAuthStore()
  const { setLocale } = useLocaleStore()
  const navigate = useNavigate()

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (loading) return
    setLoading(true)
    setError(null)
    try {
      const { token, user } = await login(email, password)
      setAuth(token, user)
      if (user.locale in locales) setLocale(user.locale as Locale)
      navigate('/dashboard')
    } catch {
      setError(t`Invalid email or password.`)
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="min-h-screen bg-bg text-text">
      <main className="max-w-md mx-auto px-4 flex flex-col justify-center min-h-screen py-12">
        <h1 className="text-xs uppercase tracking-widest text-muted mb-8"><Trans>Sign in</Trans></h1>
        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label className="block text-xs text-muted mb-1"><Trans>Email</Trans></label>
            <input
              type="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              placeholder={t`you@example.com`}
              autoComplete="email"
              required
              className={inputCls}
            />
          </div>
          <div>
            <label className="block text-xs text-muted mb-1"><Trans>Password</Trans></label>
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              autoComplete="current-password"
              required
              className={inputCls}
            />
          </div>
          {error && <p className="text-danger text-xs">{error}</p>}
          <button type="submit" disabled={loading} className={btnPrimary}>
            {loading ? <Trans>Signing in…</Trans> : <Trans>Sign in</Trans>}
          </button>
        </form>
        <p className="text-xs text-muted mt-6">
          <Trans>No account?</Trans>{' '}
          <Link to="/signup" className="text-text hover:underline"><Trans>Create one</Trans></Link>
        </p>
      </main>
    </div>
  )
}

const inputCls = 'w-full bg-surface border border-border px-3 py-2 text-xs text-text placeholder:text-muted outline-none focus:border-text transition-colors'
const btnPrimary = 'w-full py-2.5 text-xs bg-text text-bg hover:bg-muted transition-colors disabled:opacity-40 disabled:cursor-not-allowed'
