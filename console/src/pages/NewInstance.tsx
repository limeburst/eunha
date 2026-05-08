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
        : 'Failed to create instance.')
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="max-w-sm px-6 py-6">
      <h1 className="text-xs uppercase tracking-widest text-muted mb-6">New instance</h1>

      <form onSubmit={handleSubmit} className="space-y-6">
        <section className="space-y-3">
          <p className="text-xs text-muted uppercase tracking-widest border-b border-border pb-2">Domain</p>
          <div className="flex gap-0 border border-border">
            <button type="button" onClick={() => setUseCustom(false)}
              className={`px-3 py-1.5 text-xs transition-colors ${!useCustom ? 'bg-text text-bg' : 'text-muted hover:text-text'}`}>
              eunha subdomain
            </button>
            <button type="button" onClick={() => setUseCustom(true)}
              className={`px-3 py-1.5 text-xs transition-colors border-l border-border ${useCustom ? 'bg-text text-bg' : 'text-muted hover:text-text'}`}>
              Custom domain
            </button>
          </div>
          {!useCustom ? (
            <div className="flex">
              <input value={subdomain}
                onChange={(e) => setSubdomain(e.target.value.toLowerCase().replace(/[^a-z0-9-]/g, ''))}
                placeholder="myinstance"
                className="flex-1 bg-surface border border-border border-r-0 px-3 py-2 text-xs text-text placeholder:text-muted outline-none focus:border-text transition-colors"
                required={!useCustom} />
              <span className="px-3 py-2 border border-border bg-elevated text-xs text-muted select-none whitespace-nowrap">
                .{EUNHA_DOMAIN}
              </span>
            </div>
          ) : (
            <div>
              <input value={customDomain}
                onChange={(e) => setCustomDomain(e.target.value.toLowerCase())}
                placeholder="community.example.com"
                className={inputCls} required={useCustom} />
              <p className="text-xs text-muted mt-1">Point an A/CNAME record to eunha first.</p>
            </div>
          )}
        </section>

        <section className="space-y-3">
          <p className="text-xs text-muted uppercase tracking-widest border-b border-border pb-2">Instance</p>
          <div>
            <label className="block text-xs text-muted mb-1">Display name</label>
            <input value={title} onChange={(e) => setTitle(e.target.value)}
              placeholder="My Community" required className={inputCls} />
          </div>
        </section>

        <section className="space-y-3">
          <p className="text-xs text-muted uppercase tracking-widest border-b border-border pb-2">Admin account</p>
          <div>
            <label className="block text-xs text-muted mb-1">Username</label>
            <input value={adminUsername}
              onChange={(e) => setAdminUsername(e.target.value.toLowerCase().replace(/[^a-z0-9_]/g, ''))}
              placeholder="admin" required className={inputCls} />
          </div>
          <div>
            <label className="block text-xs text-muted mb-1">Email</label>
            <input type="email" value={adminEmail} onChange={(e) => setAdminEmail(e.target.value)}
              placeholder="admin@example.com" required className={inputCls} />
          </div>
          <div>
            <label className="block text-xs text-muted mb-1">Password</label>
            <input type="password" value={adminPassword} onChange={(e) => setAdminPassword(e.target.value)}
              autoComplete="new-password" required minLength={8} className={inputCls} />
          </div>
        </section>

        {error && <p className="text-danger text-xs">{error}</p>}

        <button type="submit" disabled={!valid || loading} className="w-full py-2 text-xs bg-text text-bg hover:bg-muted transition-colors disabled:opacity-40 disabled:cursor-not-allowed">
          {loading ? 'Creating…' : 'Create instance'}
        </button>
      </form>
    </div>
  )
}

const inputCls = 'w-full bg-surface border border-border px-3 py-2 text-xs text-text placeholder:text-muted outline-none focus:border-text transition-colors'
