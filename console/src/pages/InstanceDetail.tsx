import { useEffect, useState } from 'react'
import { useParams, useNavigate } from 'react-router-dom'
import { ExternalLink } from 'lucide-react'
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
      .then((inst) => {
        setInstance(inst)
        setTitle(inst.title)
      })
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

  if (loading) return <div className="px-6 py-8 text-muted text-sm">Loading…</div>
  if (error || !instance) return <div className="px-6 py-8 text-danger text-sm">{error ?? 'Not found.'}</div>

  return (
    <div className="max-w-xl mx-auto px-6 py-8 space-y-8">
      {/* Header */}
      <div className="flex items-start justify-between gap-4">
        <div>
          <h1 className="font-brand text-2xl text-text mb-1">{instance.title}</h1>
          <div className="flex items-center gap-3">
            <span className="text-sm text-muted">{instance.domain}</span>
            <StatusBadge status={instance.status} />
          </div>
        </div>
        <a
          href={`https://${instance.domain}`}
          target="_blank"
          rel="noreferrer"
          className="flex items-center gap-1.5 text-xs text-muted hover:text-accent transition-colors mt-1"
        >
          Open <ExternalLink size={12} />
        </a>
      </div>

      {/* Settings */}
      <section className="bg-surface border border-border rounded-lg p-5">
        <h2 className="text-xs font-medium text-muted uppercase tracking-wider mb-4">Settings</h2>
        <form onSubmit={handleSave} className="space-y-4">
          <div>
            <label className="block text-xs text-muted mb-1.5">Display name</label>
            <input
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              required
              className={inputCls}
            />
          </div>
          <div className="flex items-center gap-3">
            <button
              type="submit"
              disabled={saving || title === instance.title}
              className="px-4 py-2 rounded-md text-sm font-medium bg-accent text-bg hover:opacity-90 transition-opacity disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {saving ? 'Saving…' : 'Save'}
            </button>
            {saveMsg && (
              <span className={`text-xs ${saveMsg === 'Saved.' ? 'text-success' : 'text-danger'}`}>
                {saveMsg}
              </span>
            )}
          </div>
        </form>
      </section>

      {/* Danger zone */}
      <section className="border border-danger/30 rounded-lg p-5">
        <h2 className="text-xs font-medium text-danger uppercase tracking-wider mb-3">Danger zone</h2>
        <p className="text-sm text-muted mb-4">
          Permanently delete this instance and all its data. This cannot be undone.
        </p>
        <div className="space-y-2">
          <label className="block text-xs text-muted">
            Type <span className="text-text font-mono">{instance.domain}</span> to confirm
          </label>
          <input
            value={deleteConfirm}
            onChange={(e) => setDeleteConfirm(e.target.value)}
            placeholder={instance.domain}
            className={inputCls}
          />
          <button
            onClick={handleDelete}
            disabled={deleteConfirm !== instance.domain || deleting}
            className="px-4 py-2 rounded-md text-sm font-medium border border-danger text-danger hover:bg-danger/10 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
          >
            {deleting ? 'Deleting…' : 'Delete instance'}
          </button>
        </div>
      </section>
    </div>
  )
}

const inputCls = `w-full bg-elevated border border-border rounded-md px-3 py-2 text-sm text-text
  placeholder:text-muted outline-none focus:border-accent transition-colors`
