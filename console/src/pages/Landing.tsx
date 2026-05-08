import { Link } from 'react-router-dom'
import { Globe, Users, Server, ArrowRight, Zap } from 'lucide-react'

export function Landing() {
  return (
    <div className="min-h-screen bg-bg text-text flex flex-col">

      {/* Nav */}
      <nav className="flex items-center justify-between px-8 py-5 border-b border-border">
        <span className="font-brand text-xl text-accent">eunha</span>
        <div className="flex items-center gap-6">
          <a href="#features" className="text-sm text-muted hover:text-text transition-colors">Features</a>
          <Link to="/login" className="text-sm text-muted hover:text-text transition-colors">Sign in</Link>
          <Link
            to="/signup"
            className="text-sm px-4 py-2 rounded-md bg-accent text-bg font-medium hover:opacity-90 transition-opacity"
          >
            Get started
          </Link>
        </div>
      </nav>

      {/* Hero */}
      <section className="flex-1 flex flex-col items-center justify-center text-center px-6 py-24 max-w-4xl mx-auto w-full">
        <p className="text-xs font-medium tracking-widest text-accent uppercase mb-5">은하 · galaxy</p>
        <h1 className="font-brand text-5xl sm:text-6xl text-text mb-6 leading-tight">
          Fediverse hosting,<br />done simply
        </h1>
        <p className="text-muted text-lg max-w-2xl mb-10 leading-relaxed">
          eunha lets you run your own Mastodon-compatible community on the fediverse.
          No server administration, no DevOps — just a space for your people.
        </p>
        <div className="flex flex-wrap gap-3 justify-center">
          <Link
            to="/signup"
            className="flex items-center gap-2 px-6 py-3 rounded-md bg-accent text-bg text-sm font-medium hover:opacity-90 transition-opacity"
          >
            Start your instance
            <ArrowRight size={15} />
          </Link>
          <Link
            to="/login"
            className="flex items-center gap-2 px-6 py-3 rounded-md border border-border text-text text-sm hover:bg-surface transition-colors"
          >
            Sign in
          </Link>
        </div>
      </section>

      <div className="border-t border-border" />

      {/* Features */}
      <section id="features" className="grid sm:grid-cols-2 lg:grid-cols-4 divide-y sm:divide-y-0 sm:divide-x divide-border">
        <Feature icon={<Globe size={18} />} title="Fully federated">
          Compatible with Mastodon, Misskey, Pixelfed, and anything that speaks ActivityPub.
        </Feature>
        <Feature icon={<Users size={18} />} title="Your community">
          Your own domain. Your own rules. Your own member base — separate from every other instance.
        </Feature>
        <Feature icon={<Zap size={18} />} title="Ready in minutes">
          Pick a subdomain, set up your admin account, and your instance is live. No config files, no terminals.
        </Feature>
        <Feature icon={<Server size={18} />} title="Powered by eunha">
          Built on a compact Rust + PostgreSQL stack. Fast, reliable, and multi-tenant by design.
        </Feature>
      </section>

      {/* Footer */}
      <footer className="border-t border-border px-8 py-5 flex items-center justify-between text-xs text-muted">
        <span className="font-brand text-accent">eunha</span>
        <span>Fediverse hosting for communities</span>
      </footer>
    </div>
  )
}

function Feature({ icon, title, children }: { icon: React.ReactNode; title: string; children: React.ReactNode }) {
  return (
    <div className="px-8 py-10 flex flex-col gap-3">
      <div className="text-accent">{icon}</div>
      <h3 className="font-medium text-text text-sm">{title}</h3>
      <p className="text-muted text-sm leading-relaxed">{children}</p>
    </div>
  )
}
