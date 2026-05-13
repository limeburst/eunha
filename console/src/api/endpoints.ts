import { api } from './client'
import type { User, Instance, Rule, CreateInstanceRequest, InviteTree, ConsoleInvite, Application } from './types'

// ── Auth ───────────────────────────────────────────────────────────────────

export const signup = (email: string, locale: string) =>
  api.post<{ needs_confirmation: boolean; request_token: string }>('/api/console/auth/signup', { email, locale })

export const confirmAccount = (token: string, requestToken: string) =>
  api.post<{ token: string; user: User }>('/api/console/auth/confirm', { token, request_token: requestToken })

export const login = (email: string, password: string) =>
  api.post<{ token: string; user: User }>('/api/console/auth/login', { email, password })

export const getMe = () =>
  api.get<User>('/api/console/auth/me')

export const changePassword = (currentPassword: string | null, newPassword: string) =>
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

export const updateInstance = (domain: string, patch: Partial<Pick<Instance, 'title' | 'custom_domain' | 'registrations_open' | 'approval_required' | 'privacy_policy'> & { rules?: Rule[] }>) =>
  api.patch<Instance>(`/api/console/instances/${domain}`, patch)

export const uploadInstanceIcon = (domain: string, file: File) => {
  const form = new FormData()
  form.append('icon', file)
  return api.postForm<Instance>(`/api/console/instances/${domain}/icon`, form)
}

export const deleteInstance = (domain: string) =>
  api.delete<void>(`/api/console/instances/${domain}`)

export const getInviteTree = (domain: string) =>
  api.get<InviteTree>(`/api/console/instances/${domain}/invites`)

export const createConsoleInvite = (domain: string, maxUses?: number | null) =>
  api.post<ConsoleInvite>(`/api/console/instances/${domain}/invites`, { max_uses: maxUses ?? null })

export const listApplications = (domain: string) =>
  api.get<Application[]>(`/api/console/instances/${domain}/applications`)

export const approveApplication = (domain: string, accountId: string) =>
  api.post<void>(`/api/console/instances/${domain}/applications/${accountId}/approve`, {})

export const rejectApplication = (domain: string, accountId: string) =>
  api.post<void>(`/api/console/instances/${domain}/applications/${accountId}/reject`, {})
