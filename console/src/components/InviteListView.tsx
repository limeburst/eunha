import React from 'react'
import { Trans } from '@lingui/macro'
import type { InviteTreeMember, ConsoleInvite, RejectedMember } from '../api/types'

export function InviteListView({ members, invites, rejected }: { members: InviteTreeMember[]; invites: ConsoleInvite[]; rejected: RejectedMember[] }) {
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

  if (invites.length === 0 && members.length === 0 && rejected.length === 0) {
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
              <span className="text-xs text-muted flex-1 truncate">{inv.url}</span>
              <button
                onClick={() => copyUrl(inv.url, inv.id)}
                className="text-xs text-muted hover:text-text transition-colors shrink-0"
              >
                {copied === inv.id ? '✓' : 'copy'}
              </button>
            </div>
            <div className="flex items-center gap-3 text-xs text-muted/60">
              {inv.created_by_username && (
                <span>by <span className="text-muted">{inv.created_by_username}</span></span>
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
                  <span key={m.account_id} className="text-xs text-text bg-elevated px-1.5 py-0.5 border border-border">
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
            <div key={m.account_id} className="text-xs text-muted">{m.username}</div>
          ))}
        </div>
      )}
      {rejected.length > 0 && (
        <div className="space-y-1 pt-2 border-t border-border">
          <p className="text-xs text-muted/60 uppercase tracking-widest pb-1"><Trans>Rejected</Trans></p>
          {rejected.map((r) => (
            <div key={r.account_id} className="flex items-start justify-between gap-3 py-0.5">
              <div>
                <span className="text-xs text-muted line-through">@{r.username}</span>
                <span className="text-xs text-muted/50 ml-2">{r.email}</span>
                {r.reason && <p className="text-xs text-muted/50 mt-0.5">{r.reason}</p>}
              </div>
              <span className="text-xs text-muted/40 shrink-0">{new Date(r.rejected_at).toLocaleDateString()}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
