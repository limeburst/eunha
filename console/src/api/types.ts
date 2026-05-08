export interface User {
  id: string
  email: string
  created_at: string
}

export type InstanceStatus = 'provisioning' | 'running' | 'stopped' | 'error'

export interface Instance {
  id: string
  domain: string
  title: string
  status: InstanceStatus
  plan: string
  region: string
  created_at: string
  admin_account?: string
}

export interface CreateInstanceRequest {
  domain: string
  title: string
  admin_username: string
  admin_email: string
  admin_password: string
}
