import { NavLink, useNavigate } from 'react-router-dom'
import { LayoutDashboard, Plus, LogOut } from 'lucide-react'
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
      {/* Sidebar */}
      <aside className="w-52 flex-shrink-0 border-r border-border flex flex-col px-4 py-5 sticky top-0 h-screen">
        <NavLink to="/dashboard" className="font-brand text-xl text-accent mb-8 block">
          eunha
        </NavLink>

        <nav className="flex flex-col gap-1 flex-1">
          <NavLink
            to="/dashboard"
            end
            className={({ isActive }) => navCls(isActive)}
          >
            <LayoutDashboard size={16} />
            Instances
          </NavLink>
          <NavLink
            to="/instances/new"
            className={({ isActive }) => navCls(isActive)}
          >
            <Plus size={16} />
            New instance
          </NavLink>
        </nav>

        {user && (
          <div className="mt-auto pt-4 border-t border-border">
            <p className="text-xs text-muted truncate mb-3">{user.email}</p>
            <button
              onClick={handleLogout}
              className="flex items-center gap-2 text-xs text-muted hover:text-danger transition-colors"
            >
              <LogOut size={13} />
              Sign out
            </button>
          </div>
        )}
      </aside>

      {/* Main */}
      <main className="flex-1 min-w-0 overflow-auto">
        {children}
      </main>
    </div>
  )
}

const navCls = (isActive: boolean) =>
  `flex items-center gap-2.5 px-3 py-2 rounded-md text-sm transition-colors
  ${isActive ? 'bg-accent-soft text-accent' : 'text-muted hover:text-text hover:bg-elevated'}`
