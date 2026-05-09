import React, { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { Trans, t } from '@lingui/macro'
import { useLingui } from '@lingui/react'
import { instanceUserInviteTree, instanceUserChangePassword } from '../api/endpoints'
import type { InviteTree } from '../api/types'
import { useInstanceAuthStore } from '../store/instance_auth'
import { InviteListView } from '../components/InviteListView'

export function InstanceUserHome() {
  useLingui()
  const navigate = useNavigate()
  const { user, logout } = useInstanceAuthStore()

  const [inviteTree, setInviteTree] = useState<InviteTree | null>(null)

  const [currentPassword, setCurrentPassword] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [pwSaving, setPwSaving] = useState(false)
  const [pwMsg, setPwMsg] = useState<string | null>(null)
  const [pwMsgOk, setPwMsgOk] = useState(false)

  useEffect(() => {
    instanceUserInviteTree().then(setInviteTree).catch(() => {})
  }, [])

  const handleLogout = () => {
    logout()
    navigate('/my/login')
  }

  const handleChangePassword = async (e: React.FormEvent) => {
    e.preventDefault()
    if (pwSaving) return
    setPwSaving(true)
    setPwMsg(null)
    try {
      await instanceUserChangePassword(currentPassword, newPassword)
      setPwMsg(t`Password changed.`)
      setPwMsgOk(true)
      setCurrentPassword('')
      setNewPassword('')
    } catch {
      setPwMsg(t`Failed. Check your current password.`)
      setPwMsgOk(false)
    } finally {
      setPwSaving(false)
    }
  }

  return (
    <div className="min-h-screen bg-bg text-text">
      <main className="max-w-md mx-auto px-4 py-8 space-y-8">
        <div className="flex items-center justify-between">
          <span className="text-xs tracking-widest uppercase text-muted">eunha.social</span>
          <button
            onClick={handleLogout}
            className="text-xs text-muted hover:text-text transition-colors"
          >
            <Trans>Sign out</Trans>
          </button>
        </div>

        {user && (
          <div className="space-y-0.5">
            <p className="text-sm text-text font-mono">{user.username}</p>
            <p className="text-xs text-muted">{user.instance_domain}</p>
          </div>
        )}

        <section className="space-y-4">
          <p className="text-xs text-muted uppercase tracking-widest border-b border-border pb-2">
            <Trans>Members &amp; invites</Trans>
          </p>
          {inviteTree ? (
            <InviteListView members={inviteTree.members} invites={inviteTree.invites} />
          ) : (
            <p className="text-xs text-muted"><Trans>Loading…</Trans></p>
          )}
        </section>

        <section className="space-y-4">
          <p className="text-xs text-muted uppercase tracking-widest border-b border-border pb-2">
            <Trans>Change password</Trans>
          </p>
          <form onSubmit={handleChangePassword} className="space-y-3">
            <div>
              <label className="block text-xs text-muted mb-1"><Trans>Current password</Trans></label>
              <input
                type="password"
                value={currentPassword}
                onChange={(e) => setCurrentPassword(e.target.value)}
                required
                className={inputCls}
              />
            </div>
            <div>
              <label className="block text-xs text-muted mb-1"><Trans>New password</Trans></label>
              <input
                type="password"
                value={newPassword}
                onChange={(e) => setNewPassword(e.target.value)}
                required
                minLength={8}
                className={inputCls}
              />
            </div>
            <div className="flex items-center gap-3">
              <button
                type="submit"
                disabled={pwSaving || !currentPassword || !newPassword}
                className="px-3 py-1.5 text-xs border border-border text-muted hover:text-text hover:border-text transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
              >
                {pwSaving ? <Trans>Saving…</Trans> : <Trans>Change password</Trans>}
              </button>
              {pwMsg && (
                <span className={`text-xs ${pwMsgOk ? 'text-success' : 'text-danger'}`}>
                  {pwMsg}
                </span>
              )}
            </div>
          </form>
        </section>
      </main>
    </div>
  )
}

const inputCls = 'w-full bg-surface border border-border px-3 py-2 text-xs text-text placeholder:text-muted outline-none focus:border-text transition-colors'
