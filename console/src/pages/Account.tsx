import { useState } from 'react'
import { Link } from 'react-router-dom'
import { Trans, t } from '@lingui/macro'
import { useLingui } from '@lingui/react'
import { useAuthStore } from '../store/auth'
import { useLocaleStore } from '../store/locale'
import { locales, type Locale } from '../i18n'
import { changePassword } from '../api/endpoints'

export function Account() {
  useLingui()
  const { user } = useAuthStore()
  const { locale, setLocale } = useLocaleStore()

  const [current, setCurrent] = useState('')
  const [next, setNext] = useState('')
  const [confirm, setConfirm] = useState('')
  const [loading, setLoading] = useState(false)
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null)

  const mismatch = next.length > 0 && confirm.length > 0 && next !== confirm
  const valid = current.length > 0 && next.length >= 8 && next === confirm

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!valid || loading) return
    setLoading(true)
    setMsg(null)
    try {
      await changePassword(current, next)
      setMsg({ ok: true, text: t`Password changed.` })
      setCurrent('')
      setNext('')
      setConfirm('')
    } catch (err) {
      const is401 = err instanceof Error && err.message.includes('401')
      setMsg({
        ok: false,
        text: is401 ? t`Current password is incorrect.` : t`Failed to change password.`,
      })
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="space-y-8">
      <Link to="/dashboard" className="text-xs text-muted hover:text-text transition-colors">
        <Trans>← instances</Trans>
      </Link>

      <div className="space-y-1">
        <h1 className="text-xs uppercase tracking-widest text-muted"><Trans>Account</Trans></h1>
        {user && <p className="text-xs text-muted">{user.email}</p>}
      </div>

      <section className="space-y-3">
        <p className="text-xs text-muted uppercase tracking-widest border-b border-border pb-2">
          <Trans>Language</Trans>
        </p>
        <div className="flex gap-0 border border-border w-fit">
          {(Object.entries(locales) as [Locale, string][]).map(([code, label], i) => (
            <button
              key={code}
              type="button"
              onClick={() => setLocale(code)}
              className={`px-3 py-1.5 text-xs transition-colors ${i > 0 ? 'border-l border-border' : ''} ${
                locale === code ? 'bg-text text-bg' : 'text-muted hover:text-text'
              }`}
            >
              {label}
            </button>
          ))}
        </div>
      </section>

      <section className="space-y-4">
        <p className="text-xs text-muted uppercase tracking-widest border-b border-border pb-2">
          <Trans>Change password</Trans>
        </p>
        <form onSubmit={handleSubmit} className="space-y-3">
          <div>
            <label className="block text-xs text-muted mb-1"><Trans>Current password</Trans></label>
            <input
              type="password"
              value={current}
              onChange={(e) => setCurrent(e.target.value)}
              autoComplete="current-password"
              required
              className={inputCls}
            />
          </div>
          <div>
            <label className="block text-xs text-muted mb-1"><Trans>New password</Trans></label>
            <input
              type="password"
              value={next}
              onChange={(e) => setNext(e.target.value)}
              autoComplete="new-password"
              required
              minLength={8}
              className={inputCls}
            />
            <p className="text-xs text-muted mt-1"><Trans>At least 8 characters</Trans></p>
          </div>
          <div>
            <label className="block text-xs text-muted mb-1"><Trans>Confirm new password</Trans></label>
            <input
              type="password"
              value={confirm}
              onChange={(e) => setConfirm(e.target.value)}
              autoComplete="new-password"
              required
              className={inputCls}
            />
            {mismatch && <p className="text-danger text-xs mt-1"><Trans>Passwords do not match.</Trans></p>}
          </div>
          {msg && (
            <p className={`text-xs ${msg.ok ? 'text-success' : 'text-danger'}`}>{msg.text}</p>
          )}
          <button
            type="submit"
            disabled={!valid || loading}
            className="px-3 py-1.5 text-xs border border-border text-muted hover:text-text hover:border-text transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
          >
            {loading ? <Trans>Saving…</Trans> : <Trans>Change password</Trans>}
          </button>
        </form>
      </section>
    </div>
  )
}

const inputCls = 'w-full bg-surface border border-border px-3 py-2 text-xs text-text placeholder:text-muted outline-none focus:border-text transition-colors'
