import { useEffect, useState } from 'react'
import { Link } from 'react-router-dom'
import { Plus, ExternalLink } from 'lucide-react'
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
    <div className="max-w-3xl mx-auto px-6 py-8">
      <div className="flex items-center justify-between mb-8">
        <h1 className="font-brand text-2xl text-text">Instances</h1>
        <Link
          to="/instances/new"
          className="flex items-center gap-2 px-4 py-2 rounded-md bg-accent text-bg text-sm font-medium hover:opacity-90 transition-opacity"
        >
          <Plus size={15} />
          New instance
        </Link>
      </div>

      {loading && (
        <div className="text-muted text-sm py-12 text-center">Loading…</div>
      )}
      {error && (
        <div className="text-danger text-sm py-12 text-center">{error}</div>
      )}
      {!loading && !error && instances.length === 0 && (
        <div className="border border-border rounded-lg px-6 py-16 text-center">
          <p className="text-muted text-sm mb-4">You don't have any instances yet.</p>
          <Link
            to="/instances/new"
            className="inline-flex items-center gap-2 px-4 py-2 rounded-md bg-accent text-bg text-sm font-medium hover:opacity-90 transition-opacity"
          >
            <Plus size={15} />
            Create your first instance
          </Link>
        </div>
      )}
      {!loading && instances.length > 0 && (
        <div className="flex flex-col divide-y divide-border border border-border rounded-lg overflow-hidden">
          {instances.map((inst) => (
            <Link
              key={inst.id}
              to={`/instances/${inst.domain}`}
              className="flex items-center justify-between px-5 py-4 bg-surface hover:bg-elevated transition-colors"
            >
              <div className="flex flex-col gap-0.5 min-w-0">
                <span className="text-sm font-medium text-text truncate">{inst.title}</span>
                <span className="text-xs text-muted">{inst.domain}</span>
              </div>
              <div className="flex items-center gap-4 flex-shrink-0 ml-4">
                <StatusBadge status={inst.status} />
                <a
                  href={`https://${inst.domain}`}
                  target="_blank"
                  rel="noreferrer"
                  onClick={(e) => e.stopPropagation()}
                  className="text-muted hover:text-accent transition-colors"
                >
                  <ExternalLink size={14} />
                </a>
              </div>
            </Link>
          ))}
        </div>
      )}
    </div>
  )
}
