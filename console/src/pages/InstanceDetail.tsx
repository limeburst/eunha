import React, { useEffect, useState } from 'react'
import { useParams, useNavigate, Link } from 'react-router-dom'
import { Trans, t } from '@lingui/macro'
import { useLingui } from '@lingui/react'
import { getInstance, updateInstance, deleteInstance, getInviteTree, createConsoleInvite } from '../api/endpoints'
import type { Instance, InviteTree, ConsoleInvite, InviteTreeMember } from '../api/types'
import { StatusBadge } from '../components/StatusBadge'

export function InstanceDetail() {
  useLingui()
  const { domain } = useParams<{ domain: string }>()
  const navigate = useNavigate()
  const [instance, setInstance] = useState<Instance | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const [title, setTitle] = useState('')
  const [customDomain, setCustomDomain] = useState('')
  const [saving, setSaving] = useState(false)
  const [saveMsg, setSaveMsg] = useState<string | null>(null)
  const [saveMsgOk, setSaveMsgOk] = useState(false)

  const [deleteConfirm, setDeleteConfirm] = useState('')
  const [deleting, setDeleting] = useState(false)

  const [inviteTree, setInviteTree] = useState<InviteTree | null>(null)
  const [creatingInvite, setCreatingInvite] = useState(false)
  const [newInvite, setNewInvite] = useState<ConsoleInvite | null>(null)

  useEffect(() => {
    if (!domain) return
    getInstance(domain)
      .then((inst) => { setInstance(inst); setTitle(inst.title); setCustomDomain(inst.custom_domain ?? '') })
      .catch(() => setError('err'))
      .finally(() => setLoading(false))
    getInviteTree(domain).then(setInviteTree).catch(() => {})
  }, [domain])

  const handleSave = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!domain || saving) return
    setSaving(true)
    setSaveMsg(null)
    try {
      const patch: Parameters<typeof updateInstance>[1] = { title }
      const normalised = customDomain.trim().toLowerCase()
      if (normalised !== (instance?.custom_domain ?? '')) {
        patch.custom_domain = normalised || null
      }
      const updated = await updateInstance(domain, patch)
      setInstance(updated)
      setCustomDomain(updated.custom_domain ?? '')
      setSaveMsg(t`Saved.`)
      setSaveMsgOk(true)
    } catch {
      setSaveMsg(t`Failed to save.`)
      setSaveMsgOk(false)
    } finally {
      setSaving(false)
    }
  }

  const handleCreateInvite = async () => {
    if (!domain || creatingInvite) return
    setCreatingInvite(true)
    setNewInvite(null)
    try {
      const invite = await createConsoleInvite(domain)
      setNewInvite(invite)
      setInviteTree((prev) => prev ? { ...prev, invites: [invite, ...prev.invites] } : prev)
    } finally {
      setCreatingInvite(false)
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
            href={`https://${instance.custom_domain ?? instance.domain}`}
            target="_blank"
            rel="noreferrer"
            className="text-xs text-muted hover:text-text transition-colors mt-0.5 shrink-0"
          >
            ↗
          </a>
        </div>
        <div className="flex items-center gap-3">
          {instance.custom_domain ? (
            <>
              <span className="text-xs text-muted">{instance.custom_domain}</span>
              <span className="text-xs text-muted/50">{instance.domain}</span>
            </>
          ) : (
            <span className="text-xs text-muted">{instance.domain}</span>
          )}
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
          <div>
            <label className="block text-xs text-muted mb-1"><Trans>Custom domain</Trans></label>
            <input
              value={customDomain}
              onChange={(e) => setCustomDomain(e.target.value)}
              placeholder="example.com"
              className={inputCls}
            />
          </div>
          <div className="flex items-center gap-3">
            <button
              type="submit"
              disabled={saving || (title === instance.title && customDomain.trim().toLowerCase() === (instance.custom_domain ?? ''))}
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

      <section className="space-y-4">
        <div className="flex items-center justify-between border-b border-border pb-2">
          <p className="text-xs text-muted uppercase tracking-widest"><Trans>Invites</Trans></p>
          <button
            onClick={handleCreateInvite}
            disabled={creatingInvite}
            className="text-xs text-muted hover:text-text transition-colors disabled:opacity-40"
          >
            {creatingInvite ? <Trans>Generating…</Trans> : <Trans>+ Generate link</Trans>}
          </button>
        </div>

        {newInvite && (
          <div className="text-xs font-mono bg-surface border border-border px-3 py-2 text-text break-all">
            {newInvite.url}
          </div>
        )}

        {inviteTree && (
          <InviteListView members={inviteTree.members} invites={inviteTree.invites} />
        )}
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
      <Trans>← dashboard</Trans>
    </Link>
  )
}

function InviteListView({ members, invites }: { members: InviteTreeMember[]; invites: ConsoleInvite[] }) {
  const [copied, setCopied] = React.useState<string | null>(null)

  const redeemedBy: Record<string, InviteTreeMember[]> = {}
  for (const m of members) {
    if (m.invite_id) {
      ;(redeemedBy[m.invite_id] ??= []).push(m)
    }
  }

  const copyUrl = (url: string, id: string) => {
    navigator.clipboard.writeText(url).then(() => {
      setCopied(id)
      setTimeout(() => setCopied(null), 2000)
    })
  }

  if (invites.length === 0 && members.length === 0) {
    return <p className="text-xs text-muted"><Trans>No members yet.</Trans></p>
  }

  return (
    <div className="space-y-3">
      {invites.map((inv) => {
        const redeemers = redeemedBy[inv.id] ?? []
        const isExpired = inv.expires_at ? new Date(inv.expires_at) < new Date() : false
        const isMaxed = inv.max_uses != null && inv.uses >= inv.max_uses
        return (
          <div key={inv.id} className="border border-border p-2 space-y-1.5">
            <div className="flex items-center gap-2">
              <span className="text-xs font-mono text-muted flex-1 truncate">{inv.url}</span>
              <button
                onClick={() => copyUrl(inv.url, inv.id)}
                className="text-xs text-muted hover:text-text transition-colors shrink-0"
              >
                {copied === inv.id ? '✓' : 'copy'}
              </button>
            </div>
            <div className="flex items-center gap-3 text-xs text-muted/60">
              {inv.created_by_username && (
                <span>by <span className="font-mono text-muted">{inv.created_by_username}</span></span>
              )}
              <span>
                {inv.uses}{inv.max_uses != null ? `/${inv.max_uses}` : ''} used
              </span>
              {isExpired && <span className="text-danger">expired</span>}
              {!isExpired && isMaxed && <span className="text-muted">maxed</span>}
            </div>
            {redeemers.length > 0 && (
              <div className="flex flex-wrap gap-1.5 pt-0.5">
                {redeemers.map((m) => (
                  <span key={m.account_id} className="text-xs font-mono text-text bg-elevated px-1.5 py-0.5 border border-border">
                    {m.username}
                  </span>
                ))}
              </div>
            )}
          </div>
        )
      })}
      {members.some((m) => !m.invite_id) && (
        <div className="space-y-0.5">
          {members.filter((m) => !m.invite_id).map((m) => (
            <div key={m.account_id} className="text-xs font-mono text-muted">{m.username}</div>
          ))}
        </div>
      )}
    </div>
  )
}

const inputCls = 'w-full bg-surface border border-border px-3 py-2 text-xs text-text placeholder:text-muted outline-none focus:border-text transition-colors'
