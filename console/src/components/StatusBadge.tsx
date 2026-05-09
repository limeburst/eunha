import { t } from '@lingui/macro'
import { useLingui } from '@lingui/react'
import type { InstanceStatus } from '../api/types'

export function StatusBadge({ status }: { status: InstanceStatus }) {
  useLingui() // re-render on locale change
  const labels: Record<InstanceStatus, string> = {
    provisioning: t`provisioning`,
    running:      t`running`,
    stopped:      t`stopped`,
    error:        t`error`,
  }
  const colors: Record<InstanceStatus, string> = {
    provisioning: 'text-muted border-muted',
    running:      'text-success border-success',
    stopped:      'text-muted border-border',
    error:        'text-danger border-danger',
  }
  return (
    <span className={`text-xs px-1.5 py-0.5 border ${colors[status]}`}>
      {labels[status]}
    </span>
  )
}
