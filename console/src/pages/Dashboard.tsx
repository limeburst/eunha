import { useEffect, useState } from 'react'
import { Link } from 'react-router-dom'
import { Trans } from '@lingui/macro'
import { listInstances } from '../api/endpoints'
import type { Instance } from '../api/types'
import { StatusBadge } from '../components/StatusBadge'

export function Dashboard() {
  const [instances, setInstances] = useState<Instance[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    listInstances()
      .then(setInstances)
      .catch(() => setError('err'))
      .finally(() => setLoading(false))
  }, [])

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-xs uppercase tracking-widest text-muted"><Trans>Instances</Trans></h1>
        <Link
          to="/instances/new"
          className="text-xs border border-border px-3 py-1.5 text-muted hover:text-text hover:border-text transition-colors"
        >
          <Trans>+ New</Trans>
        </Link>
      </div>

      {loading && <p className="text-muted text-xs"><Trans>Loading…</Trans></p>}
      {error && <p className="text-danger text-xs"><Trans>Failed to load instances.</Trans></p>}

      {!loading && !error && instances.length === 0 && (
        <div className="border border-border px-5 py-12 text-center space-y-4">
          <p className="text-muted text-xs"><Trans>No instances yet.</Trans></p>
          <Link
            to="/instances/new"
            className="inline-block text-xs border border-border px-3 py-1.5 text-muted hover:text-text hover:border-text transition-colors"
          >
            <Trans>Create your first instance</Trans>
          </Link>
        </div>
      )}

      {!loading && instances.length > 0 && (
        <div className="border border-border divide-y divide-border">
          {instances.map((inst) => (
            <Link
              key={inst.id}
              to={`/instances/${inst.domain}`}
              className="flex items-center justify-between px-4 py-3 hover:bg-surface transition-colors"
            >
              <div className="min-w-0">
                <p className="text-xs text-text truncate">{inst.title}</p>
                <p className="text-xs text-muted mt-0.5">{inst.domain}</p>
              </div>
              <StatusBadge status={inst.status} />
            </Link>
          ))}
        </div>
      )}
    </div>
  )
}
