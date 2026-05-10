import { useEffect, useState } from 'react'
import { Link } from 'react-router-dom'
import { Trans, t } from '@lingui/macro'
import { useLingui } from '@lingui/react'
import { useAuthStore } from '../store/auth'
import { useLocaleStore } from '../store/locale'
import { locales, type Locale } from '../i18n'
import { listInstances, changePassword, getMe } from '../api/endpoints'
import type { Instance } from '../api/types'
import { StatusBadge } from '../components/StatusBadge'

export function Dashboard() {
  useLingui()
  const { user, setUser } = useAuthStore()
  const { locale, setLocale } = useLocaleStore()

  const [instances, setInstances] = useState<Instance[]>([])
  const [loadingInstances, setLoadingInstances] = useState(true)
  const [instancesError, setInstancesError] = useState<string | null>(null)

  const [current, setCurrent] = useState('')
  const [next, setNext] = useState('')
  const [confirm, setConfirm] = useState('')
  const [savingPassword, setSavingPassword] = useState(false)
  const [passwordMsg, setPasswordMsg] = useState<{ ok: boolean; text: string } | null>(null)

  const hasPassword = user?.has_password ?? true
  const mismatch = next.length > 0 && confirm.length > 0 && next !== confirm
  const passwordValid = (hasPassword ? current.length > 0 : true) && next.length >= 8 && next === confirm

  useEffect(() => {
    getMe().then(setUser).catch(() => {})
    listInstances()
      .then(setInstances)
      .catch(() => setInstancesError('err'))
      .finally(() => setLoadingInstances(false))
  }, [])

  const handlePasswordSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!passwordValid || savingPassword) return
    setSavingPassword(true)
    setPasswordMsg(null)
    try {
      await changePassword(hasPassword ? current : null, next)
      setPasswordMsg({ ok: true, text: t`Password saved.` })
      setCurrent('')
      setNext('')
      setConfirm('')
      // Refresh user so has_password updates
      const updated = await getMe()
      setUser(updated)
    } catch (err) {
      const is401 = err instanceof Error && err.message.includes('401')
      setPasswordMsg({
        ok: false,
        text: is401 ? t`Current password is incorrect.` : t`Failed to save password.`,
      })
    } finally {
      setSavingPassword(false)
    }
  }

  return (
    <div className="space-y-10">

      {/* ── No-password banner ── */}
      {!hasPassword && (
        <div className="border border-border px-4 py-3 text-xs text-muted flex items-center justify-between gap-4">
          <span><Trans>Set a password to enable email + password login.</Trans></span>
          <a href="#set-password" className="text-text underline underline-offset-2 whitespace-nowrap">
            <Trans>Set password</Trans>
          </a>
        </div>
      )}

      {/* ── Instances ── */}
      <section className="space-y-4">
        <div className="flex items-center justify-between">
          <h1 className="text-xs uppercase tracking-widest text-muted"><Trans>Instances</Trans></h1>
          <Link
            to="/instances/new"
            className="text-xs border border-border px-3 py-1.5 text-muted hover:text-text hover:border-text transition-colors"
          >
            <Trans>+ New</Trans>
          </Link>
        </div>

        {loadingInstances && <p className="text-muted text-xs"><Trans>Loading…</Trans></p>}
        {instancesError && <p className="text-danger text-xs"><Trans>Failed to load instances.</Trans></p>}

        {!loadingInstances && !instancesError && instances.length === 0 && (
          <div className="border border-border px-5 py-12 text-center space-y-4">
            <p className="text-muted text-xs"><Trans>No instances yet.</Trans></p>
            <Link
              to="/instances/new"
              className="inline-block text-xs border border-border px-3 py-1.5 text-muted hover:text-text hover:border-text transition-colors"
            >
              <Trans>Create your first instance</Trans>
            </Link>
          </div>
        )}

        {!loadingInstances && instances.length > 0 && (
          <div className="border border-border divide-y divide-border">
            {instances.map((inst) => (
              <Link
                key={inst.id}
                to={`/instances/${inst.domain}`}
                className="flex items-center justify-between px-4 py-3 hover:bg-surface transition-colors"
              >
                <div className="min-w-0">
                  <p className="text-xs text-text truncate">{inst.title}</p>
                  <p className="text-xs text-muted mt-0.5">{inst.domain}</p>
                </div>
                <StatusBadge status={inst.status} />
              </Link>
            ))}
          </div>
        )}
      </section>

      {/* ── Account ── */}
      <section className="space-y-6">
        <div className="border-b border-border pb-2">
          <p className="text-xs text-muted uppercase tracking-widest"><Trans>Account</Trans></p>
          {user && <p className="text-xs text-muted/60 mt-1">{user.email}</p>}
        </div>

        <div className="space-y-3">
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
        </div>

        <div id="set-password" className="space-y-4">
          <p className="text-xs text-muted uppercase tracking-widest border-b border-border pb-2">
            {hasPassword ? <Trans>Change password</Trans> : <Trans>Set password</Trans>}
          </p>
          <form onSubmit={handlePasswordSubmit} className="space-y-3">
            {hasPassword && (
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
            )}
            <div>
              <label className="block text-xs text-muted mb-1">
                {hasPassword ? <Trans>New password</Trans> : <Trans>Password</Trans>}
              </label>
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
            {passwordMsg && (
              <p className={`text-xs ${passwordMsg.ok ? 'text-success' : 'text-danger'}`}>{passwordMsg.text}</p>
            )}
            <button
              type="submit"
              disabled={!passwordValid || savingPassword}
              className="px-3 py-1.5 text-xs border border-border text-muted hover:text-text hover:border-text transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
            >
              {savingPassword
                ? <Trans>Saving…</Trans>
                : hasPassword ? <Trans>Change password</Trans> : <Trans>Set password</Trans>}
            </button>
          </form>
        </div>
      </section>

    </div>
  )
}

const inputCls = 'w-full bg-surface border border-border px-3 py-2 text-xs text-text placeholder:text-muted outline-none focus:border-text transition-colors'
