import { useState } from 'react'
import { Link, useNavigate } from 'react-router-dom'
import { useAuthStore } from '../store/auth'
import { signup } from '../api/endpoints'

export function Signup() {
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
      const { token, user } = await signup(email, password)
      setAuth(token, user)
      navigate('/instances/new')
    } catch (err) {
      setError(err instanceof Error && err.message.includes('409')
        ? 'An account with that email already exists.'
        : 'Sign up failed. Please try again.')
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-bg px-4">
      <div className="w-full max-w-sm">
        <Link to="/" className="block font-brand text-3xl text-accent text-center mb-2">eunha</Link>
        <p className="text-center text-muted text-sm mb-8">Create your hosting account</p>

        <form onSubmit={handleSubmit} className="bg-surface border border-border rounded-lg p-6 space-y-4">
          <div>
            <label className="block text-xs text-muted mb-1.5">Email</label>
            <input
              type="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              placeholder="you@example.com"
              autoComplete="email"
              required
              className={inputCls}
            />
          </div>
          <div>
            <label className="block text-xs text-muted mb-1.5">Password</label>
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              autoComplete="new-password"
              required
              minLength={8}
              className={inputCls}
            />
            <p className="text-xs text-muted mt-1.5">At least 8 characters</p>
          </div>

          {error && <p className="text-danger text-sm">{error}</p>}

          <button type="submit" disabled={loading} className={btnCls}>
            {loading ? 'Creating account…' : 'Create account'}
          </button>
        </form>

        <p className="text-center text-xs text-muted mt-6">
          Already have an account?{' '}
          <Link to="/login" className="text-accent hover:underline">Sign in</Link>
        </p>
      </div>
    </div>
  )
}

const inputCls = `w-full bg-elevated border border-border rounded-md px-3 py-2 text-sm text-text
  placeholder:text-muted outline-none focus:border-accent transition-colors`

const btnCls = `w-full py-2.5 rounded-md text-sm font-medium
  bg-accent text-bg hover:opacity-90 transition-opacity
  disabled:opacity-50 disabled:cursor-not-allowed`
