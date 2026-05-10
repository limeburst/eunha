import { useState } from 'react'
import { Link, useNavigate } from 'react-router-dom'
import { Trans, t } from '@lingui/macro'
import { useLingui } from '@lingui/react'
import { useLocaleStore } from '../store/locale'
import { signup } from '../api/endpoints'

export function Signup() {
  useLingui()
  const [email, setEmail] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const { locale } = useLocaleStore()
  const navigate = useNavigate()

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (loading) return
    setLoading(true)
    setError(null)
    try {
      const { request_token } = await signup(email, locale)
      navigate(`/confirm-account?request_token=${request_token}`, { state: { email } })
    } catch (err) {
      setError(t`Sign up failed. Please try again.`)
    } finally {
      setLoading(false)
    }
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
