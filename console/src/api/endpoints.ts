import { api } from './client'
import type { User, Instance, CreateInstanceRequest } from './types'

// ── Auth ───────────────────────────────────────────────────────────────────

export const signup = (email: string, password: string) =>
  api.post<{ token: string; user: User }>('/api/console/auth/signup', { email, password })

export const login = (email: string, password: string) =>
  api.post<{ token: string; user: User }>('/api/console/auth/login', { email, password })

export const getMe = () =>
  api.get<User>('/api/console/auth/me')

export const changePassword = (currentPassword: string, newPassword: string) =>
  api.patch<void>('/api/console/auth/password', {
    current_password: currentPassword,
    new_password: newPassword,
  })

export const setLocale = (locale: string) =>
  api.patch<void>('/api/console/auth/locale', { locale })

// ── Instances ──────────────────────────────────────────────────────────────

export const listInstances = () =>
  api.get<Instance[]>('/api/console/instances')

export const getInstance = (domain: string) =>
  api.get<Instance>(`/api/console/instances/${domain}`)

export const createInstance = (req: CreateInstanceRequest) =>
  api.post<Instance>('/api/console/instances', req)

export const updateInstance = (domain: string, patch: Partial<Pick<Instance, 'title'>>) =>
  api.patch<Instance>(`/api/console/instances/${domain}`, patch)

export const deleteInstance = (domain: string) =>
  api.delete<void>(`/api/console/instances/${domain}`)
