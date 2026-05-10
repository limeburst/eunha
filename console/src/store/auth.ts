import { create } from 'zustand'
import type { User } from '../api/types'

interface AuthState {
  token: string | null
  user: User | null
  setAuth: (token: string, user: User) => void
  setUser: (user: User) => void
  logout: () => void
}

export const useAuthStore = create<AuthState>((set) => ({
  token: localStorage.getItem('console_token'),
  user: (() => {
    const raw = localStorage.getItem('console_user')
    return raw ? (JSON.parse(raw) as User) : null
  })(),

  setAuth: (token, user) => {
    localStorage.setItem('console_token', token)
    localStorage.setItem('console_user', JSON.stringify(user))
    set({ token, user })
  },

  setUser: (user) => {
    localStorage.setItem('console_user', JSON.stringify(user))
    set({ user })
  },

  logout: () => {
    localStorage.removeItem('console_token')
    localStorage.removeItem('console_user')
    set({ token: null, user: null })
  },
}))
