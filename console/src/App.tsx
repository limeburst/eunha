import { Routes, Route, Navigate } from 'react-router-dom'
import { ConsoleLayout } from './components/ConsoleLayout'
import { Landing } from './pages/Landing'
import { Login } from './pages/Login'
import { Signup } from './pages/Signup'
import { Dashboard } from './pages/Dashboard'
import { NewInstance } from './pages/NewInstance'
import { InstanceDetail } from './pages/InstanceDetail'
import { useAuthStore } from './store/auth'

function RequireAuth({ children }: { children: React.ReactNode }) {
  const { token } = useAuthStore()
  if (!token) return <Navigate to="/login" replace />
  return <>{children}</>
}

export default function App() {
  return (
    <Routes>
      <Route path="/" element={<Landing />} />
      <Route path="/login" element={<Login />} />
      <Route path="/signup" element={<Signup />} />
      <Route
        path="/*"
        element={
          <RequireAuth>
            <ConsoleLayout>
              <Routes>
                <Route path="/dashboard" element={<Dashboard />} />
                <Route path="/instances/new" element={<NewInstance />} />
                <Route path="/instances/:domain" element={<InstanceDetail />} />
                <Route path="*" element={<Navigate to="/dashboard" replace />} />
              </Routes>
            </ConsoleLayout>
          </RequireAuth>
        }
      />
    </Routes>
  )
}
