import type { InstanceStatus } from '../api/types'

const config: Record<InstanceStatus, { label: string; cls: string }> = {
  provisioning: { label: 'Provisioning', cls: 'text-accent bg-accent-soft' },
  running:      { label: 'Running',      cls: 'text-success bg-success/10' },
  stopped:      { label: 'Stopped',      cls: 'text-muted bg-elevated' },
  error:        { label: 'Error',        cls: 'text-danger bg-danger/10' },
}

export function StatusBadge({ status }: { status: InstanceStatus }) {
  const { label, cls } = config[status]
  return (
    <span className={`text-xs px-2 py-0.5 rounded-full font-medium ${cls}`}>
      {label}
    </span>
  )
}
