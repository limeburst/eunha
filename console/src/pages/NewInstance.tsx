import { useState } from 'react'
import { Link, useNavigate } from 'react-router-dom'
import { Trans, t } from '@lingui/macro'
import { useLingui } from '@lingui/react'
import { createInstance } from '../api/endpoints'

const EUNHA_DOMAIN = 'eunha.social'

export function NewInstance() {
  useLingui()
  const navigate = useNavigate()
  const [subdomain, setSubdomain] = useState('')
  const [title, setTitle] = useState('')
  const [adminUsername, setAdminUsername] = useState('')
  const [adminEmail, setAdminEmail] = useState('')
  const [adminPassword, setAdminPassword] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const domain = `${subdomain.trim()}.${EUNHA_DOMAIN}`
  const valid =
    subdomain.trim() &&
    title.trim() &&
    adminUsername.trim() &&
    adminEmail.trim() &&
    adminPassword.length >= 8

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!valid || loading) return
    setLoading(true)
    setError(null)
    try {
      await createInstance({
        domain,
        title: title.trim(),
        admin_username: adminUsername.trim(),
        admin_email: adminEmail.trim(),
        admin_password: adminPassword,
      })
      navigate('/dashboard')
    } catch (err) {
      setError(
        err instanceof Error && err.message.includes('409')
          ? t`That domain is already taken.`
          : t`Failed to create instance.`
      )
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="space-y-8">
      <Link to="/dashboard" className="text-xs text-muted hover:text-text transition-colors">
        <Trans>← instances</Trans>
      </Link>

      <h1 className="text-xs uppercase tracking-widest text-muted"><Trans>New instance</Trans></h1>

      <form onSubmit={handleSubmit} className="space-y-6">
        <section className="space-y-3">
          <p className="text-xs text-muted uppercase tracking-widest border-b border-border pb-2">
            <Trans>Domain</Trans>
          </p>
          <div className="flex">
            <input
              value={subdomain}
              onChange={(e) =>
                setSubdomain(e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, ''))
              }
              placeholder={t`myinstance`}
              className="flex-1 bg-surface border border-border border-r-0 px-3 py-2 text-xs text-text placeholder:text-muted outline-none focus:border-text transition-colors"
              required
            />
            <span className="px-3 py-2 border border-border bg-elevated text-xs text-muted select-none whitespace-nowrap">
              .{EUNHA_DOMAIN}
            </span>
          </div>
        </section>

        <section className="space-y-3">
          <p className="text-xs text-muted uppercase tracking-widest border-b border-border pb-2">
            <Trans>Instance</Trans>
          </p>
          <div>
            <label className="block text-xs text-muted mb-1"><Trans>Display name</Trans></label>
            <input
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder={t`My Community`}
              required
              className={inputCls}
            />
          </div>
        </section>

        <section className="space-y-3">
          <p className="text-xs text-muted uppercase tracking-widest border-b border-border pb-2">
            <Trans>Admin account</Trans>
          </p>
          <div>
            <label className="block text-xs text-muted mb-1"><Trans>Username</Trans></label>
            <input
              value={adminUsername}
              onChange={(e) =>
                setAdminUsername(e.target.value.toLowerCase().replace(/[^a-z0-9_]/g, ''))
              }
              placeholder="admin"
              required
              className={inputCls}
            />
          </div>
          <div>
            <label className="block text-xs text-muted mb-1"><Trans>Email</Trans></label>
            <input
              type="email"
              value={adminEmail}
              onChange={(e) => setAdminEmail(e.target.value)}
              placeholder="admin@example.com"
              required
              className={inputCls}
            />
          </div>
          <div>
            <label className="block text-xs text-muted mb-1"><Trans>Password</Trans></label>
            <input
              type="password"
              value={adminPassword}
              onChange={(e) => setAdminPassword(e.target.value)}
              autoComplete="new-password"
              required
              minLength={8}
              className={inputCls}
            />
            <p className="text-xs text-muted mt-1"><Trans>At least 8 characters</Trans></p>
          </div>
        </section>

        {error && <p className="text-danger text-xs">{error}</p>}

        <button
          type="submit"
          disabled={!valid || loading}
          className="w-full py-2.5 text-xs bg-text text-bg hover:bg-muted transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
        >
          {loading ? <Trans>Creating…</Trans> : <Trans>Create instance</Trans>}
        </button>
      </form>
    </div>
  )
}

const inputCls =
  'w-full bg-surface border border-border px-3 py-2 text-xs text-text placeholder:text-muted outline-none focus:border-text transition-colors'
