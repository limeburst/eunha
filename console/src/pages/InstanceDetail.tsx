import { useEffect, useState } from 'react'
import { useParams, useNavigate } from 'react-router-dom'
import { getInstance, updateInstance, deleteInstance } from '../api/endpoints'
import type { Instance } from '../api/types'
import { StatusBadge } from '../components/StatusBadge'

export function InstanceDetail() {
  const { domain } = useParams<{ domain: string }>()
  const navigate = useNavigate()
  const [instance, setInstance] = useState<Instance | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const [title, setTitle] = useState('')
  const [saving, setSaving] = useState(false)
  const [saveMsg, setSaveMsg] = useState<string | null>(null)

  const [deleteConfirm, setDeleteConfirm] = useState('')
  const [deleting, setDeleting] = useState(false)

  useEffect(() => {
    if (!domain) return
    getInstance(domain)
      .then((inst) => { setInstance(inst); setTitle(inst.title) })
      .catch(() => setError('Instance not found.'))
      .finally(() => setLoading(false))
  }, [domain])

  const handleSave = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!domain || saving) return
    setSaving(true)
    setSaveMsg(null)
    try {
      const updated = await updateInstance(domain, { title })
      setInstance(updated)
      setSaveMsg('Saved.')
    } catch {
      setSaveMsg('Failed to save.')
    } finally {
      setSaving(false)
    }
  }

  const handleDelete = async () => {
    if (!domain || deleting || deleteConfirm !== domain) return
    setDeleting(true)
    try {
      await deleteInstance(domain)
      navigate('/dashboard')
    } catch {
      setDeleting(false)
    }
  }

  if (loading) return <div className="px-6 py-6 text-muted text-xs">Loading…</div>
  if (error || !instance) return <div className="px-6 py-6 text-danger text-xs">{error ?? 'Not found.'}</div>

  return (
    <div className="max-w-sm px-6 py-6 space-y-8">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-sm text-text mb-1">{instance.title}</h1>
          <div className="flex items-center gap-3">
            <span className="text-xs text-muted">{instance.domain}</span>
            <StatusBadge status={instance.status} />
          </div>
        </div>
        <a href={`https://${instance.domain}`} target="_blank" rel="noreferrer"
          className="text-xs text-muted hover:text-text transition-colors mt-0.5">↗</a>
      </div>

      <section className="space-y-4">
        <p className="text-xs text-muted uppercase tracking-widest border-b border-border pb-2">Settings</p>
        <form onSubmit={handleSave} className="space-y-3">
          <div>
            <label className="block text-xs text-muted mb-1">Display name</label>
            <input value={title} onChange={(e) => setTitle(e.target.value)} required className={inputCls} />
          </div>
          <div className="flex items-center gap-3">
            <button type="submit" disabled={saving || title === instance.title} className="px-3 py-1.5 text-xs border border-border text-muted hover:text-text hover:border-text transition-colors disabled:opacity-40 disabled:cursor-not-allowed">
              {saving ? 'Saving…' : 'Save'}
            </button>
            {saveMsg && <span className={`text-xs ${saveMsg === 'Saved.' ? 'text-success' : 'text-danger'}`}>{saveMsg}</span>}
          </div>
        </form>
      </section>

      <section className="space-y-3">
        <p className="text-xs text-danger uppercase tracking-widest border-b border-danger/30 pb-2">Danger zone</p>
        <p className="text-xs text-muted">Permanently delete this instance and all its data.</p>
        <div className="space-y-2">
          <label className="block text-xs text-muted">
            Type <span className="text-text font-mono">{instance.domain}</span> to confirm
          </label>
          <input value={deleteConfirm} onChange={(e) => setDeleteConfirm(e.target.value)}
            placeholder={instance.domain} className={inputCls} />
          <button onClick={handleDelete} disabled={deleteConfirm !== instance.domain || deleting}
            className="px-3 py-1.5 text-xs border border-danger text-danger hover:bg-danger/10 transition-colors disabled:opacity-40 disabled:cursor-not-allowed">
            {deleting ? 'Deleting…' : 'Delete instance'}
          </button>
        </div>
      </section>
    </div>
  )
}

const inputCls = 'w-full bg-surface border border-border px-3 py-2 text-xs text-text placeholder:text-muted outline-none focus:border-text transition-colors'
