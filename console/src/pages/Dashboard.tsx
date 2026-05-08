import { useEffect, useState } from 'react'
import { Link } from 'react-router-dom'
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
      .catch(() => setError('Failed to load instances.'))
      .finally(() => setLoading(false))
  }, [])

  return (
    <div className="max-w-2xl px-6 py-6">
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-xs uppercase tracking-widest text-muted">Instances</h1>
        <Link to="/instances/new" className="text-xs border border-border px-3 py-1.5 text-muted hover:text-text hover:border-text transition-colors">
          + New
        </Link>
      </div>

      {loading && <p className="text-muted text-xs">Loading…</p>}
      {error && <p className="text-danger text-xs">{error}</p>}

      {!loading && !error && instances.length === 0 && (
        <div className="border border-border px-5 py-10 text-center">
          <p className="text-muted text-xs mb-4">No instances.</p>
          <Link to="/instances/new" className="text-xs border border-border px-3 py-1.5 text-muted hover:text-text hover:border-text transition-colors">
            Create instance
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
