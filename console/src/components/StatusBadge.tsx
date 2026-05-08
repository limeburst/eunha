import type { InstanceStatus } from '../api/types'

const config: Record<InstanceStatus, { label: string; cls: string }> = {
  provisioning: { label: 'provisioning', cls: 'text-muted border-muted' },
  running:      { label: 'running',      cls: 'text-success border-success' },
  stopped:      { label: 'stopped',      cls: 'text-muted border-border' },
  error:        { label: 'error',        cls: 'text-danger border-danger' },
}

export function StatusBadge({ status }: { status: InstanceStatus }) {
  const { label, cls } = config[status]
  return (
    <span className={`text-xs px-1.5 py-0.5 border ${cls}`}>
      {label}
    </span>
  )
}
