import { create } from 'zustand'
import type { InstanceUser } from '../api/types'

interface InstanceAuthState {
  token: string | null
  user: InstanceUser | null
  setAuth: (token: string, user: InstanceUser) => void
  logout: () => void
}

export const useInstanceAuthStore = create<InstanceAuthState>((set) => ({
  token: localStorage.getItem('instance_user_token'),
  user: (() => {
    const raw = localStorage.getItem('instance_user')
    return raw ? (JSON.parse(raw) as InstanceUser) : null
  })(),

  setAuth: (token, user) => {
    localStorage.setItem('instance_user_token', token)
    localStorage.setItem('instance_user', JSON.stringify(user))
    set({ token, user })
  },

  logout: () => {
    localStorage.removeItem('instance_user_token')
    localStorage.removeItem('instance_user')
    set({ token: null, user: null })
  },
}))
