import { useState } from 'react'
import { Link, useNavigate } from 'react-router-dom'
import { Trans, t } from '@lingui/macro'
import { useLingui } from '@lingui/react'
import { useAuthStore } from '../store/auth'
import { useLocaleStore } from '../store/locale'
import { signup } from '../api/endpoints'

export function Signup() {
  useLingui()
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [needsConfirmation, setNeedsConfirmation] = useState(false)
  const { setAuth } = useAuthStore()
  const { locale, setLocale } = useLocaleStore()
  const navigate = useNavigate()

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (loading) return
    setLoading(true)
    setError(null)
    try {
      const result = await signup(email, password)
      if ('needs_confirmation' in result && result.needs_confirmation) {
        setNeedsConfirmation(true)
      } else if ('token' in result && 'user' in result) {
        setAuth(result.token, result.user)
        setLocale(locale)
        navigate('/dashboard')
      }
    } catch (err) {
      setError(
        err instanceof Error && err.message.includes('409')
          ? t`An account with that email already exists.`
          : t`Sign up failed. Please try again.`
      )
    } finally {
      setLoading(false)
    }
  }

  if (needsConfirmation) {
    return (
      <div className="min-h-screen bg-bg text-text">
        <main className="max-w-md mx-auto px-4 flex flex-col justify-center min-h-screen py-12">
          <h1 className="text-xs uppercase tracking-widest text-muted mb-4"><Trans>Create account</Trans></h1>
          <p className="text-sm text-text mb-2">Check your email to confirm your account.</p>
          <p className="text-xs text-muted">We sent a confirmation link to <strong>{email}</strong>. Click the link in the email to activate your account.</p>
        </main>
      </div>
    )
  }

  return (
    <div className="min-h-screen bg-bg text-text">
      <main className="max-w-md mx-auto px-4 flex flex-col justify-center min-h-screen py-12">
        <h1 className="text-xs uppercase tracking-widest text-muted mb-8"><Trans>Create account</Trans></h1>
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
              autoComplete="new-password"
              required
              minLength={8}
              className={inputCls}
            />
            <p className="text-xs text-muted mt-1"><Trans>At least 8 characters</Trans></p>
          </div>
          {error && <p className="text-danger text-xs">{error}</p>}
          <button type="submit" disabled={loading} className={btnPrimary}>
            {loading ? <Trans>Creating account…</Trans> : <Trans>Create account</Trans>}
          </button>
        </form>
        <p className="text-xs text-muted mt-6">
          <Trans>Already have an account?</Trans>{' '}
          <Link to="/login" className="text-text hover:underline"><Trans>Sign in</Trans></Link>
        </p>
      </main>
    </div>
  )
}

const inputCls = 'w-full bg-surface border border-border px-3 py-2 text-xs text-text placeholder:text-muted outline-none focus:border-text transition-colors'
const btnPrimary = 'w-full py-2.5 text-xs bg-text text-bg hover:bg-muted transition-colors disabled:opacity-40 disabled:cursor-not-allowed'
