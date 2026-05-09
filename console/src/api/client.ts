function createApi(getToken: () => string | null) {
  async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
    const token = getToken()
    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
      ...(options.headers as Record<string, string>),
    }
    if (token) headers['Authorization'] = `Bearer ${token}`

    const res = await fetch(path, { ...options, headers })
    if (!res.ok) {
      const body = await res.text().catch(() => res.statusText)
      throw new Error(`${res.status}: ${body}`)
    }
    if (res.status === 204) return undefined as T
    return res.json() as Promise<T>
  }

  return {
    get:    <T>(path: string)                 => request<T>(path),
    post:   <T>(path: string, body?: unknown) => request<T>(path, { method: 'POST',  body: body ? JSON.stringify(body) : undefined }),
    patch:  <T>(path: string, body?: unknown) => request<T>(path, { method: 'PATCH', body: body ? JSON.stringify(body) : undefined }),
    delete: <T>(path: string)                 => request<T>(path, { method: 'DELETE' }),
  }
}

export const api = createApi(() => localStorage.getItem('console_token'))
