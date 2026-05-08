import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { createInstance } from '../api/endpoints'

const EUNHA_DOMAIN = 'eunha.social'

export function NewInstance() {
  const navigate = useNavigate()
  const [subdomain, setSubdomain] = useState('')
  const [customDomain, setCustomDomain] = useState('')
  const [useCustom, setUseCustom] = useState(false)
  const [title, setTitle] = useState('')
  const [adminUsername, setAdminUsername] = useState('')
  const [adminEmail, setAdminEmail] = useState('')
  const [adminPassword, setAdminPassword] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const domain = useCustom ? customDomain.trim() : `${subdomain.trim()}.${EUNHA_DOMAIN}`
  const valid = title.trim() && domain && adminUsername.trim() && adminEmail.trim() && adminPassword.length >= 8

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
      setError(err instanceof Error && err.message.includes('409')
        ? 'That domain is already taken.'
        : 'Failed to create instance. Please try again.')
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="max-w-xl mx-auto px-6 py-8">
      <h1 className="font-brand text-2xl text-text mb-1">New instance</h1>
      <p className="text-muted text-sm mb-8">Your instance will be ready in a few moments.</p>

      <form onSubmit={handleSubmit} className="space-y-6">
        {/* Domain */}
        <section className="bg-surface border border-border rounded-lg p-5 space-y-4">
          <h2 className="text-xs font-medium text-muted uppercase tracking-wider">Domain</h2>

          <div className="flex gap-2">
            <button
              type="button"
              onClick={() => setUseCustom(false)}
              className={tabCls(!useCustom)}
            >
              eunha subdomain
            </button>
            <button
              type="button"
              onClick={() => setUseCustom(true)}
              className={tabCls(useCustom)}
            >
              Custom domain
            </button>
          </div>

          {!useCustom ? (
            <div className="flex items-center gap-0">
              <input
                value={subdomain}
                onChange={(e) => setSubdomain(e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, ''))}
                placeholder="myinstance"
                className={`flex-1 rounded-l-md rounded-r-none border-r-0 ${inputCls}`}
                required={!useCustom}
              />
              <span className="px-3 py-2 bg-elevated border border-border rounded-r-md text-sm text-muted select-none">
                .{EUNHA_DOMAIN}
              </span>
            </div>
          ) : (
            <div className="space-y-1.5">
              <input
                value={customDomain}
                onChange={(e) => setCustomDomain(e.target.value.toLowerCase())}
                placeholder="community.example.com"
                className={inputCls}
                required={useCustom}
              />
              <p className="text-xs text-muted">
                Point an A/CNAME record to eunha before submitting.
              </p>
            </div>
          )}
        </section>

        {/* Instance details */}
        <section className="bg-surface border border-border rounded-lg p-5 space-y-4">
          <h2 className="text-xs font-medium text-muted uppercase tracking-wider">Instance</h2>
          <Field label="Display name">
            <input
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="My Community"
              required
              className={inputCls}
            />
          </Field>
        </section>

        {/* Admin account */}
        <section className="bg-surface border border-border rounded-lg p-5 space-y-4">
          <h2 className="text-xs font-medium text-muted uppercase tracking-wider">Admin account</h2>
          <Field label="Username">
            <input
              value={adminUsername}
              onChange={(e) => setAdminUsername(e.target.value.toLowerCase().replace(/[^a-z0-9_]/g, ''))}
              placeholder="admin"
              required
              className={inputCls}
            />
          </Field>
          <Field label="Email">
            <input
              type="email"
              value={adminEmail}
              onChange={(e) => setAdminEmail(e.target.value)}
              placeholder="admin@example.com"
              required
              className={inputCls}
            />
          </Field>
          <Field label="Password">
            <input
              type="password"
              value={adminPassword}
              onChange={(e) => setAdminPassword(e.target.value)}
              autoComplete="new-password"
              required
              minLength={8}
              className={inputCls}
            />
          </Field>
        </section>

        {error && <p className="text-danger text-sm">{error}</p>}

        <button
          type="submit"
          disabled={!valid || loading}
          className="w-full py-2.5 rounded-md text-sm font-medium bg-accent text-bg hover:opacity-90 transition-opacity disabled:opacity-50 disabled:cursor-not-allowed"
        >
          {loading ? 'Creating instance…' : 'Create instance'}
        </button>
      </form>
    </div>
  )
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <label className="block text-xs text-muted mb-1.5">{label}</label>
      {children}
    </div>
  )
}

const inputCls = `w-full bg-elevated border border-border rounded-md px-3 py-2 text-sm text-text
  placeholder:text-muted outline-none focus:border-accent transition-colors`

const tabCls = (active: boolean) =>
  `px-3 py-1.5 rounded-md text-xs font-medium transition-colors
  ${active ? 'bg-accent-soft text-accent' : 'text-muted hover:text-text hover:bg-elevated'}`
