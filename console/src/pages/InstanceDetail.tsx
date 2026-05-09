import { useEffect, useState } from 'react'
import { useParams, useNavigate, Link } from 'react-router-dom'
import { Trans, t } from '@lingui/macro'
import { useLingui } from '@lingui/react'
import { getInstance, updateInstance, deleteInstance } from '../api/endpoints'
import type { Instance } from '../api/types'
import { StatusBadge } from '../components/StatusBadge'

export function InstanceDetail() {
  useLingui()
  const { domain } = useParams<{ domain: string }>()
  const navigate = useNavigate()
  const [instance, setInstance] = useState<Instance | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const [title, setTitle] = useState('')
  const [saving, setSaving] = useState(false)
  const [saveMsg, setSaveMsg] = useState<string | null>(null)
  const [saveMsgOk, setSaveMsgOk] = useState(false)

  const [deleteConfirm, setDeleteConfirm] = useState('')
  const [deleting, setDeleting] = useState(false)

  useEffect(() => {
    if (!domain) return
    getInstance(domain)
      .then((inst) => { setInstance(inst); setTitle(inst.title) })
      .catch(() => setError('err'))
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
      setSaveMsg(t`Saved.`)
      setSaveMsgOk(true)
    } catch {
      setSaveMsg(t`Failed to save.`)
      setSaveMsgOk(false)
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

  if (loading) return (
    <div className="space-y-6">
      <Back />
      <p className="text-muted text-xs"><Trans>Loading…</Trans></p>
    </div>
  )

  if (error || !instance) return (
    <div className="space-y-6">
      <Back />
      <p className="text-danger text-xs">{error ? <Trans>Failed to load instances.</Trans> : <Trans>Not found.</Trans>}</p>
    </div>
  )

  return (
    <div className="space-y-8">
      <Back />

      <div className="space-y-1">
        <div className="flex items-start justify-between gap-3">
          <h1 className="text-sm text-text">{instance.title}</h1>
          <a
            href={`https://${instance.domain}`}
            target="_blank"
            rel="noreferrer"
            className="text-xs text-muted hover:text-text transition-colors mt-0.5 shrink-0"
          >
            ↗
          </a>
        </div>
        <div className="flex items-center gap-3">
          <span className="text-xs text-muted">{instance.domain}</span>
          <StatusBadge status={instance.status} />
        </div>
      </div>

      <section className="space-y-4">
        <p className="text-xs text-muted uppercase tracking-widest border-b border-border pb-2">
          <Trans>Settings</Trans>
        </p>
        <form onSubmit={handleSave} className="space-y-3">
          <div>
            <label className="block text-xs text-muted mb-1"><Trans>Display name</Trans></label>
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
              className="px-3 py-1.5 text-xs border border-border text-muted hover:text-text hover:border-text transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
            >
              {saving ? <Trans>Saving…</Trans> : <Trans>Save</Trans>}
            </button>
            {saveMsg && (
              <span className={`text-xs ${saveMsgOk ? 'text-success' : 'text-danger'}`}>
                {saveMsg}
              </span>
            )}
          </div>
        </form>
      </section>

      <section className="space-y-3">
        <p className="text-xs text-danger uppercase tracking-widest border-b border-danger/30 pb-2">
          <Trans>Danger zone</Trans>
        </p>
        <p className="text-xs text-muted">
          <Trans>Permanently delete this instance and all its data.</Trans>
        </p>
        <div className="space-y-2">
          <label className="block text-xs text-muted">
            <Trans>Type <span className="text-text font-mono">{instance.domain}</span> to confirm</Trans>
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
            className="px-3 py-1.5 text-xs border border-danger text-danger hover:bg-danger/10 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
          >
            {deleting ? <Trans>Deleting…</Trans> : <Trans>Delete instance</Trans>}
          </button>
        </div>
      </section>
    </div>
  )
}

function Back() {
  return (
    <Link to="/dashboard" className="text-xs text-muted hover:text-text transition-colors">
      <Trans>← instances</Trans>
    </Link>
  )
}

const inputCls = 'w-full bg-surface border border-border px-3 py-2 text-xs text-text placeholder:text-muted outline-none focus:border-text transition-colors'
