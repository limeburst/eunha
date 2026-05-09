export interface User {
  id: string
  email: string
  locale: string
  created_at: string
}

export type InstanceStatus = 'provisioning' | 'running' | 'stopped' | 'error'

export interface Instance {
  id: string
  domain: string
  custom_domain?: string | null
  title: string
  status: InstanceStatus
  plan: string
  region: string
  created_at: string
  admin_account?: string
}

export interface ConsoleInvite {
  id: string
  code: string
  url: string
  created_by_account_id: string | null
  created_by_username: string | null
  max_uses: number | null
  uses: number
  expires_at: string | null
  created_at: string
}

export interface InviteTreeMember {
  account_id: string
  username: string
  invite_id: string | null
  invited_by_account_id: string | null
  invited_by_username: string | null
  joined_at: string
}

export interface InviteTree {
  members: InviteTreeMember[]
  invites: ConsoleInvite[]
}

export interface CreateInstanceRequest {
  domain: string
  custom_domain?: string
  title: string
  admin_username: string
  admin_email: string
  admin_password: string
}
