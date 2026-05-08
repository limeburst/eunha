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
    <div className="min-h-screen flex flex-col bg-bg text-text">
      <header className="border-b border-border px-6 py-4">
        <Link to="/" className="text-text text-sm tracking-widest uppercase">eunha</Link>
      </header>
      <div className="flex-1 flex items-center justify-center px-4">
        <div className="w-full max-w-xs">
          <h1 className="text-xs uppercase tracking-widest text-muted mb-6">Sign in</h1>
          <form onSubmit={handleSubmit} className="space-y-4">
            <div>
              <label className="block text-xs text-muted mb-1">Email</label>
              <input type="email" value={email} onChange={(e) => setEmail(e.target.value)}
                placeholder="you@example.com" autoComplete="email" required className={inputCls} />
            </div>
            <div>
              <label className="block text-xs text-muted mb-1">Password</label>
              <input type="password" value={password} onChange={(e) => setPassword(e.target.value)}
                autoComplete="current-password" required className={inputCls} />
            </div>
            {error && <p className="text-danger text-xs">{error}</p>}
            <button type="submit" disabled={loading} className={btnPrimary}>
              {loading ? 'Signing in…' : 'Sign in'}
            </button>
          </form>
          <p className="text-xs text-muted mt-6">
            No account?{' '}
            <Link to="/signup" className="text-text hover:underline">Create one</Link>
          </p>
        </div>
      </div>
    </div>
  )
}

const inputCls = 'w-full bg-surface border border-border px-3 py-2 text-xs text-text placeholder:text-muted outline-none focus:border-text transition-colors'
const btnPrimary = 'w-full py-2 text-xs bg-text text-bg hover:bg-muted transition-colors disabled:opacity-40 disabled:cursor-not-allowed'
