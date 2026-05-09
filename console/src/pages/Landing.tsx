import { Link } from 'react-router-dom'
import { Trans } from '@lingui/macro'
import { useAuthStore } from '../store/auth'

export function Landing() {
  const { token } = useAuthStore()

  return (
    <div className="min-h-screen bg-bg text-text">
      <main className="max-w-md mx-auto px-4 flex flex-col justify-center min-h-screen py-16">
        <p className="text-xs text-muted uppercase tracking-widest mb-3">eunha.social</p>
        <h1 className="text-2xl text-text mb-8 leading-snug">
          <Trans>fediverse instance hosting</Trans>
        </h1>
        <div className="flex gap-3">
          {token ? (
            <Link to="/dashboard" className={btnPrimary}><Trans>Go to console</Trans></Link>
          ) : (
            <>
              <Link to="/signup" className={btnPrimary}><Trans>Create account</Trans></Link>
              <Link to="/login" className={btnSecondary}><Trans>Sign in</Trans></Link>
            </>
          )}
        </div>
        <p className="mt-8 text-xs text-muted">
          <Trans>Already a member of an instance?</Trans>{' '}
          <Link to="/my/login" className="hover:text-text transition-colors underline underline-offset-2">
            <Trans>Sign in here</Trans>
          </Link>
        </p>
      </main>
    </div>
  )
}

const btnPrimary = 'px-4 py-2 text-xs bg-text text-bg hover:bg-muted transition-colors'
const btnSecondary = 'px-4 py-2 text-xs border border-border text-muted hover:text-text hover:border-text transition-colors'
