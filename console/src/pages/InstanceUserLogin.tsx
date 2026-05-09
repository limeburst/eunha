import React, { useState } from 'react'
import { useNavigate, Link } from 'react-router-dom'
import { Trans, t } from '@lingui/macro'
import { useLingui } from '@lingui/react'
import { instanceUserLogin } from '../api/endpoints'
import { useInstanceAuthStore } from '../store/instance_auth'

export function InstanceUserLogin() {
  useLingui()
  const navigate = useNavigate()
  const { setAuth } = useInstanceAuthStore()

  const [domain, setDomain] = useState('')
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (loading) return
    setLoading(true)
    setError(null)
    try {
      const { token, user } = await instanceUserLogin(domain.trim().toLowerCase(), email, password)
      setAuth(token, user)
      navigate('/my')
    } catch {
      setError(t`Invalid domain, email, or password.`)
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="min-h-screen bg-bg text-text">
      <main className="max-w-md mx-auto px-4 py-16 space-y-8">
        <div>
          <Link to="/" className="text-xs text-muted hover:text-text transition-colors">
            ← eunha.social
          </Link>
        </div>

        <div className="space-y-1">
          <h1 className="text-sm text-text"><Trans>Sign in as instance member</Trans></h1>
          <p className="text-xs text-muted"><Trans>Use your Fediverse account credentials.</Trans></p>
        </div>

        <form onSubmit={handleSubmit} className="space-y-3">
          <div>
            <label className="block text-xs text-muted mb-1"><Trans>Instance domain</Trans></label>
            <input
              value={domain}
              onChange={(e) => setDomain(e.target.value)}
              placeholder="example.com"
              required
              autoCapitalize="none"
              className={inputCls}
            />
          </div>
          <div>
            <label className="block text-xs text-muted mb-1"><Trans>Email</Trans></label>
            <input
              type="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              required
              autoCapitalize="none"
              className={inputCls}
            />
          </div>
          <div>
            <label className="block text-xs text-muted mb-1"><Trans>Password</Trans></label>
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              required
              className={inputCls}
            />
          </div>

          {error && <p className="text-xs text-danger">{error}</p>}

          <button
            type="submit"
            disabled={loading}
            className="w-full px-4 py-2 text-xs bg-text text-bg hover:bg-muted transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
          >
            {loading ? <Trans>Signing in…</Trans> : <Trans>Sign in</Trans>}
          </button>
        </form>
      </main>
    </div>
  )
}

const inputCls = 'w-full bg-surface border border-border px-3 py-2 text-xs text-text placeholder:text-muted outline-none focus:border-text transition-colors'
