import { Link } from 'react-router-dom'

export function Landing() {
  return (
    <div className="min-h-screen flex flex-col bg-bg text-text">
      <header className="border-b border-border px-6 py-4 flex items-center justify-between">
        <span className="text-text text-sm tracking-widest uppercase">eunha.social</span>
        <Link to="/login" className="text-muted text-xs hover:text-text transition-colors">Sign in</Link>
      </header>

      <main className="flex-1 flex flex-col justify-center px-6 py-16 max-w-sm">
        <p className="text-xs text-muted uppercase tracking-widest mb-4">Console</p>
        <h1 className="text-2xl text-text mb-8 leading-snug">
          fediverse instance hosting
        </h1>
        <div className="flex gap-3">
          <Link to="/signup" className={btnPrimary}>Create account</Link>
          <Link to="/login" className={btnSecondary}>Sign in</Link>
        </div>
      </main>
    </div>
  )
}

const btnPrimary = 'px-4 py-2 text-xs bg-text text-bg hover:bg-muted transition-colors'
const btnSecondary = 'px-4 py-2 text-xs border border-border text-muted hover:text-text hover:border-text transition-colors'
