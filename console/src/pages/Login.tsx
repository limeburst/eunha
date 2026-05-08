import { useState } from 'react'
import { Link, useNavigate } from 'react-router-dom'
import { useAuthStore } from '../store/auth'
import { login } from '../api/endpoints'

export function Login() {
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const { setAuth } = useAuthStore()
  const navigate = useNavigate()

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (loading) return
    setLoading(true)
    setError(null)
    try {
      const { token, user } = await login(email, password)
      setAuth(token, user)
      navigate('/dashboard')
    } catch {
      setError('Invalid email or password.')
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-bg px-4">
      <div className="w-full max-w-sm">
        <Link to="/" className="block font-brand text-3xl text-accent text-center mb-2">eunha</Link>
        <p className="text-center text-muted text-sm mb-8">Sign in to your console</p>

        <form onSubmit={handleSubmit} className="bg-surface border border-border rounded-lg p-6 space-y-4">
          <Field label="Email">
            <input
              type="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              placeholder="you@example.com"
              autoComplete="email"
              required
              className={inputCls}
            />
          </Field>
          <Field label="Password">
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              autoComplete="current-password"
              required
              className={inputCls}
            />
          </Field>

          {error && <p className="text-danger text-sm">{error}</p>}

          <button type="submit" disabled={loading} className={btnCls}>
            {loading ? 'Signing in…' : 'Sign in'}
          </button>
        </form>

        <p className="text-center text-xs text-muted mt-6">
          No account?{' '}
          <Link to="/signup" className="text-accent hover:underline">Get started</Link>
        </p>
      </div>
    </div>
  )
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <label className="block text-xs text-muted mb-1.5">{label}</label>
      {children}
    </div>
  )
}

const inputCls = `w-full bg-elevated border border-border rounded-md px-3 py-2 text-sm text-text
  placeholder:text-muted outline-none focus:border-accent transition-colors`

const btnCls = `w-full py-2.5 rounded-md text-sm font-medium
  bg-accent text-bg hover:opacity-90 transition-opacity
  disabled:opacity-50 disabled:cursor-not-allowed`
