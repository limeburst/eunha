import { NavLink, useNavigate } from 'react-router-dom'
import { useAuthStore } from '../store/auth'

export function ConsoleLayout({ children }: { children: React.ReactNode }) {
  const { user, logout } = useAuthStore()
  const navigate = useNavigate()

  const handleLogout = () => {
    logout()
    navigate('/login')
  }

  return (
    <div className="flex h-full">
      <aside className="w-44 flex-shrink-0 border-r border-border flex flex-col px-0 py-0 sticky top-0 h-screen">
        <div className="border-b border-border px-4 py-4">
          <NavLink to="/dashboard" className="text-text text-xs tracking-widest uppercase">eunha.social</NavLink>
        </div>

        <nav className="flex flex-col flex-1 py-2">
          <NavLink to="/dashboard" end className={({ isActive }) => navCls(isActive)}>
            Instances
          </NavLink>
          <NavLink to="/instances/new" className={({ isActive }) => navCls(isActive)}>
            New instance
          </NavLink>
        </nav>

        {user && (
          <div className="border-t border-border px-4 py-3">
            <p className="text-xs text-muted truncate mb-2">{user.email}</p>
            <button onClick={handleLogout} className="text-xs text-muted hover:text-danger transition-colors">
              Sign out
            </button>
          </div>
        )}
      </aside>

      <main className="flex-1 min-w-0 overflow-auto">
        {children}
      </main>
    </div>
  )
}

const navCls = (isActive: boolean) =>
  `block px-4 py-2 text-xs transition-colors ${isActive ? 'text-text bg-elevated' : 'text-muted hover:text-text hover:bg-surface'}`
