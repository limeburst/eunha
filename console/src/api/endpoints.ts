import { api, instanceApi } from './client'
import type { User, InstanceUser, Instance, CreateInstanceRequest, InviteTree, ConsoleInvite } from './types'

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

// ── Instance user auth ─────────────────────────────────────────────────────

export const instanceUserLogin = (domain: string, email: string, password: string) =>
  api.post<{ token: string; user: InstanceUser }>('/api/console/instance_auth/login', { domain, email, password })

export const instanceUserMe = () =>
  instanceApi.get<InstanceUser>('/api/console/instance_auth/me')

export const instanceUserChangePassword = (currentPassword: string, newPassword: string) =>
  instanceApi.patch<void>('/api/console/instance_auth/password', {
    current_password: currentPassword,
    new_password: newPassword,
  })

export const instanceUserInviteTree = () =>
  instanceApi.get<InviteTree>('/api/console/instance_auth/invite_tree')

// ── Instances ──────────────────────────────────────────────────────────────

export const listInstances = () =>
  api.get<Instance[]>('/api/console/instances')

export const getInstance = (domain: string) =>
  api.get<Instance>(`/api/console/instances/${domain}`)

export const createInstance = (req: CreateInstanceRequest) =>
  api.post<Instance>('/api/console/instances', req)

export const updateInstance = (domain: string, patch: Partial<Pick<Instance, 'title' | 'custom_domain'>>) =>
  api.patch<Instance>(`/api/console/instances/${domain}`, patch)

export const deleteInstance = (domain: string) =>
  api.delete<void>(`/api/console/instances/${domain}`)

export const getInviteTree = (domain: string) =>
  api.get<InviteTree>(`/api/console/instances/${domain}/invites`)

export const createConsoleInvite = (domain: string) =>
  api.post<ConsoleInvite>(`/api/console/instances/${domain}/invites`, {})
