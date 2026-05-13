import React, { useEffect, useRef, useState } from 'react'
import { useParams, useNavigate, Link } from 'react-router-dom'
import { Trans, t } from '@lingui/macro'
import { useLingui } from '@lingui/react'
import { getInstance, updateInstance, uploadInstanceIcon, deleteInstance, getInviteTree, createConsoleInvite, listApplications, approveApplication, rejectApplication } from '../api/endpoints'
import type { Instance, Rule, InviteTree, ConsoleInvite, Application } from '../api/types'
import { StatusBadge } from '../components/StatusBadge'
import { InviteListView } from '../components/InviteListView'

export function InstanceDetail() {
  useLingui()
  const { domain } = useParams<{ domain: string }>()
  const navigate = useNavigate()
  const [instance, setInstance] = useState<Instance | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const [title, setTitle] = useState('')
  const [customDomain, setCustomDomain] = useState('')
  const [privacyPolicy, setPrivacyPolicy] = useState('')
  const [rules, setRules] = useState<Rule[]>([])
  const [saving, setSaving] = useState(false)
  const [saveMsg, setSaveMsg] = useState<string | null>(null)
  const [saveMsgOk, setSaveMsgOk] = useState(false)

  const [iconUploading, setIconUploading] = useState(false)
  const [iconMsg, setIconMsg] = useState<string | null>(null)
  const [iconMsgOk, setIconMsgOk] = useState(false)
  const iconInputRef = useRef<HTMLInputElement>(null)

  const [deleteConfirm, setDeleteConfirm] = useState('')
  const [deleting, setDeleting] = useState(false)

  const [inviteTree, setInviteTree] = useState<InviteTree | null>(null)
  const [creatingInvite, setCreatingInvite] = useState(false)
  const [newInvite, setNewInvite] = useState<ConsoleInvite | null>(null)
  const [inviteMaxUses, setInviteMaxUses] = useState('')

  const [applications, setApplications] = useState<Application[]>([])
  const [appActing, setAppActing] = useState<string | null>(null)

  useEffect(() => {
    if (!domain) return
    getInstance(domain)
      .then((inst) => {
        setInstance(inst)
        setTitle(inst.title)
        setCustomDomain(inst.custom_domain ?? '')
        setPrivacyPolicy(inst.privacy_policy ?? '')
        setRules(inst.rules ?? [])
      })
      .catch(() => setError('err'))
      .finally(() => setLoading(false))
    getInviteTree(domain).then(setInviteTree).catch(() => {})
    listApplications(domain).then(setApplications).catch(() => {})
  }, [domain])

  const handleSave = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!domain || saving) return
    setSaving(true)
    setSaveMsg(null)
    try {
      const patch: Parameters<typeof updateInstance>[1] = { title, privacy_policy: privacyPolicy, rules }
      const normalised = customDomain.trim().toLowerCase()
      if (normalised !== (instance?.custom_domain ?? '')) {
        patch.custom_domain = normalised || null
      }
      const updated = await updateInstance(domain, patch)
      setInstance(updated)
      setCustomDomain(updated.custom_domain ?? '')
      setPrivacyPolicy(updated.privacy_policy ?? '')
      setRules(updated.rules ?? [])
      setSaveMsg(t`Saved.`)
      setSaveMsgOk(true)
    } catch {
      setSaveMsg(t`Failed to save.`)
      setSaveMsgOk(false)
    } finally {
      setSaving(false)
    }
  }

  const handleIconUpload = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file || !domain || iconUploading) return
    setIconUploading(true)
    setIconMsg(null)
    try {
      const updated = await uploadInstanceIcon(domain, file)
      setInstance(updated)
      setIconMsg(t`Icon updated.`)
      setIconMsgOk(true)
    } catch {
      setIconMsg(t`Failed to upload icon.`)
      setIconMsgOk(false)
    } finally {
      setIconUploading(false)
      if (iconInputRef.current) iconInputRef.current.value = ''
    }
  }

  const handleToggle = async (field: 'registrations_open' | 'approval_required', value: boolean) => {
    if (!domain || !instance) return
    if (field === 'approval_required' && !value && applications.length > 0) {
      const confirmed = window.confirm(
        t`Turning off approval will immediately approve all ${applications.length} pending application(s). Continue?`
      )
      if (!confirmed) return
    }
    try {
      const updated = await updateInstance(domain, { [field]: value })
      setInstance(updated)
      if (field === 'approval_required' && !value) {
        setApplications([])
      }
    } catch {
      // silently ignore — the toggle will snap back on next render
    }
  }

  const handleApprove = async (accountId: string) => {
    if (!domain || appActing) return
    setAppActing(accountId)
    try {
      await approveApplication(domain, accountId)
      setApplications((prev) => prev.filter((a) => a.account_id !== accountId))
    } finally {
      setAppActing(null)
    }
  }

  const handleReject = async (accountId: string) => {
    if (!domain || appActing) return
    setAppActing(accountId)
    try {
      await rejectApplication(domain, accountId)
      setApplications((prev) => prev.filter((a) => a.account_id !== accountId))
    } finally {
      setAppActing(null)
    }
  }

  const handleCreateInvite = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!domain || creatingInvite) return
    setCreatingInvite(true)
    setNewInvite(null)
    const maxUses = inviteMaxUses.trim() ? parseInt(inviteMaxUses, 10) : null
    try {
      const invite = await createConsoleInvite(domain, maxUses)
      setNewInvite(invite)
      setInviteTree((prev) => prev ? { ...prev, invites: [invite, ...prev.invites] } : prev)
      setInviteMaxUses('')
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

        <div className="space-y-2">
          <label className="block text-xs text-muted"><Trans>Instance icon</Trans></label>
          <div className="flex items-center gap-4">
            {instance.icon_url ? (
              <img src={instance.icon_url} alt="" className="w-12 h-12 object-cover border border-border" />
            ) : (
              <div className="w-12 h-12 border border-border bg-surface flex items-center justify-center text-muted/40 text-xs">
                ?
              </div>
            )}
            <div className="space-y-1">
              <button
                type="button"
                onClick={() => iconInputRef.current?.click()}
                disabled={iconUploading}
                className="px-3 py-1.5 text-xs border border-border text-muted hover:text-text hover:border-text transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
              >
                {iconUploading ? <Trans>Uploading…</Trans> : <Trans>Upload icon</Trans>}
              </button>
              <input
                ref={iconInputRef}
                type="file"
                accept="image/*"
                className="hidden"
                onChange={handleIconUpload}
              />
              {iconMsg && (
                <p className={`text-xs ${iconMsgOk ? 'text-success' : 'text-danger'}`}>{iconMsg}</p>
              )}
            </div>
          </div>
        </div>

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
          <div>
            <label className="block text-xs text-muted mb-1"><Trans>Privacy policy</Trans></label>
            <textarea
              value={privacyPolicy}
              onChange={(e) => setPrivacyPolicy(e.target.value)}
              rows={6}
              placeholder={t`Enter your instance's privacy policy…`}
              className="w-full bg-surface border border-border px-3 py-2 text-xs text-text placeholder:text-muted outline-none focus:border-text transition-colors resize-y"
            />
          </div>
          <div>
            <label className="block text-xs text-muted mb-2"><Trans>Rules</Trans></label>
            <div className="space-y-2">
              {rules.map((rule, i) => (
                <div key={i} className="border border-border p-2 space-y-1">
                  <div className="flex items-start gap-2">
                    <span className="text-xs text-muted/60 mt-1.5 shrink-0 w-4">{i + 1}.</span>
                    <div className="flex-1 space-y-1">
                      <input
                        value={rule.text}
                        onChange={(e) => setRules(rules.map((r, j) => j === i ? { ...r, text: e.target.value } : r))}
                        placeholder={t`Rule text`}
                        className={inputCls}
                      />
                      <input
                        value={rule.hint}
                        onChange={(e) => setRules(rules.map((r, j) => j === i ? { ...r, hint: e.target.value } : r))}
                        placeholder={t`Hint (optional)`}
                        className={inputCls}
                      />
                    </div>
                    <button
                      type="button"
                      onClick={() => setRules(rules.filter((_, j) => j !== i))}
                      className="text-xs text-muted hover:text-danger transition-colors mt-1 shrink-0"
                    >
                      ×
                    </button>
                  </div>
                </div>
              ))}
              <button
                type="button"
                onClick={() => setRules([...rules, { text: '', hint: '' }])}
                className="text-xs text-muted hover:text-text transition-colors"
              >
                + <Trans>Add rule</Trans>
              </button>
            </div>
          </div>
          <div className="flex items-center gap-3">
            <button
              type="submit"
              disabled={saving}
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

        <div className="space-y-2 pt-1">
          <Toggle
            label={t`Open registrations`}
            description={t`Allow new users to sign up`}
            checked={instance.registrations_open}
            onChange={(v) => handleToggle('registrations_open', v)}
          />
          <Toggle
            label={t`Require approval`}
            description={t`New sign-ups must be approved before they can log in`}
            checked={instance.approval_required}
            onChange={(v) => handleToggle('approval_required', v)}
          />
        </div>
      </section>

      {instance.approval_required && (
        <section className="space-y-4">
          <p className="text-xs text-muted uppercase tracking-widest border-b border-border pb-2">
            <Trans>Applications</Trans>
            {applications.length > 0 && (
              <span className="ml-2 text-text">{applications.length}</span>
            )}
          </p>
          {applications.length === 0 ? (
            <p className="text-xs text-muted"><Trans>No pending applications.</Trans></p>
          ) : (
            <ul className="space-y-3">
              {applications.map((app) => (
                <li key={app.account_id} className="border border-border p-3 space-y-2">
                  <div className="flex items-center justify-between gap-3">
                    <div>
                      <span className="text-xs text-text font-medium">@{app.username}</span>
                      <span className="text-xs text-muted ml-2">{app.email}</span>
                    </div>
                    <div className="flex gap-2 shrink-0">
                      <button
                        onClick={() => handleApprove(app.account_id)}
                        disabled={appActing === app.account_id}
                        className="px-2 py-1 text-xs border border-success text-success hover:bg-success/10 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
                      >
                        <Trans>Approve</Trans>
                      </button>
                      <button
                        onClick={() => handleReject(app.account_id)}
                        disabled={appActing === app.account_id}
                        className="px-2 py-1 text-xs border border-danger text-danger hover:bg-danger/10 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
                      >
                        <Trans>Reject</Trans>
                      </button>
                    </div>
                  </div>
                  {app.reason && (
                    <p className="text-xs text-muted border-t border-border pt-2">{app.reason}</p>
                  )}
                  <p className="text-xs text-muted/60">{new Date(app.applied_at).toLocaleString()}</p>
                </li>
              ))}
            </ul>
          )}
        </section>
      )}

      <section className="space-y-4">
        <div className="flex items-center justify-between border-b border-border pb-2">
          <p className="text-xs text-muted uppercase tracking-widest"><Trans>Invites</Trans></p>
          <form onSubmit={handleCreateInvite} className="flex items-center gap-2">
            <input
              type="number"
              min={1}
              value={inviteMaxUses}
              onChange={(e) => setInviteMaxUses(e.target.value)}
              placeholder={t`unlimited`}
              className="w-24 bg-surface border border-border px-2 py-1 text-xs text-text placeholder:text-muted outline-none focus:border-text transition-colors"
            />
            <button
              type="submit"
              disabled={creatingInvite}
              className="text-xs text-muted hover:text-text transition-colors disabled:opacity-40 shrink-0"
            >
              {creatingInvite ? <Trans>Generating…</Trans> : <Trans>+ Generate link</Trans>}
            </button>
          </form>
        </div>

        {newInvite && (
          <div className="text-xs bg-surface border border-border px-3 py-2 text-text break-all">
            {newInvite.url}
            {newInvite.max_uses != null && (
              <span className="ml-2 text-muted">({newInvite.max_uses} use{newInvite.max_uses !== 1 ? 's' : ''})</span>
            )}
          </div>
        )}

        {inviteTree && (
          <InviteListView members={inviteTree.members} invites={inviteTree.invites} rejected={inviteTree.rejected ?? []} />
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
            <Trans>Type <span className="text-text font-semibold">{instance.domain}</span> to confirm</Trans>
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

function Toggle({ label, description, checked, onChange }: {
  label: string
  description: string
  checked: boolean
  onChange: (v: boolean) => void
}) {
  return (
    <label className="flex items-center justify-between gap-3 cursor-pointer py-1">
      <div>
        <span className="text-xs text-text">{label}</span>
        <p className="text-xs text-muted">{description}</p>
      </div>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        onClick={() => onChange(!checked)}
        className={`relative inline-flex h-5 w-9 items-center rounded-full transition-colors shrink-0 ${checked ? 'bg-text' : 'bg-border'}`}
      >
        <span
          className={`inline-block h-3.5 w-3.5 rounded-full bg-surface transition-transform ${checked ? 'translate-x-4' : 'translate-x-0.5'}`}
        />
      </button>
    </label>
  )
}

const inputCls = 'w-full bg-surface border border-border px-3 py-2 text-xs text-text placeholder:text-muted outline-none focus:border-text transition-colors'
