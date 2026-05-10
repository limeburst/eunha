import { useState } from 'react'
import { useNavigate, useSearchParams } from 'react-router-dom'
import { Trans, t } from '@lingui/macro'
import { useLingui } from '@lingui/react'
import { useAuthStore } from '../store/auth'
import { useLocaleStore } from '../store/locale'
import { confirmAccount } from '../api/endpoints'

export function ConfirmAccount() {
  useLingui()
  const [searchParams] = useSearchParams()
  const [code, setCode] = useState(searchParams.get('token') ?? '')
  const requestToken = searchParams.get('request_token') ?? ''
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const { setAuth } = useAuthStore()
  const { locale, setLocale } = useLocaleStore()
  const navigate = useNavigate()

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (loading) return
    if (!requestToken) {
      setError(t`This confirmation code is invalid or has already been used.`)
      return
    }
    setLoading(true)
    setError(null)
    try {
      const { token: sessionToken, user } = await confirmAccount(code.trim(), requestToken)
      setAuth(sessionToken, user)
      setLocale(locale)
      navigate('/dashboard')
    } catch (err) {
      setError(
        err instanceof Error && err.message.includes('404')
          ? t`This confirmation code is invalid or has already been used.`
          : t`Sign up failed. Please try again.`
      )
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="min-h-screen bg-bg text-text">
      <main className="max-w-md mx-auto px-4 flex flex-col justify-center min-h-screen py-12">
        <h1 className="text-xs uppercase tracking-widest text-muted mb-8"><Trans>Confirm your account</Trans></h1>
        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label className="block text-xs text-muted mb-1"><Trans>Confirmation code</Trans></label>
            <input
              type="text"
              inputMode="numeric"
              value={code}
              onChange={(e) => setCode(e.target.value.replace(/\D/g, '').slice(0, 6))}
              placeholder="000000"
              required
              maxLength={6}
              className={`${inputCls} tracking-widest text-center text-base`}
            />
          </div>
          {error && <p className="text-danger text-xs">{error}</p>}
          <button type="submit" disabled={loading || code.length < 6} className={btnPrimary}>
            {loading ? <Trans>Confirming…</Trans> : <Trans>Confirm account</Trans>}
          </button>
        </form>
      </main>
    </div>
  )
}

const inputCls = 'w-full bg-surface border border-border px-3 py-2 text-xs text-text placeholder:text-muted outline-none focus:border-text transition-colors'
const btnPrimary = 'w-full py-2.5 text-xs bg-text text-bg hover:bg-muted transition-colors disabled:opacity-40 disabled:cursor-not-allowed'
