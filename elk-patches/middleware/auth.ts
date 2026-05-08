import type { RouteLocationNormalized } from 'vue-router'

export default defineNuxtRouteMiddleware((to) => {
  if (import.meta.server)
    return

  if (to.path === '/signin/callback')
    return

  if (isHydrated.value)
    return handleAuth(to)

  onHydrated(() => handleAuth(to))
})

function handleAuth(to: RouteLocationNormalized) {
  // Use the injected instance domain (set by axum from Host header) when available,
  // so unauthenticated visitors land on the correct instance's local timeline.
  const server = (typeof window !== 'undefined' && (window as any).__eunha_instance) || currentServer.value

  if (to.path === '/') {
    // Installed PWA shortcut to notifications
    if (to.query['notifications-pwa-shortcut'] !== undefined) {
      if (currentUser.value)
        return navigateTo('/notifications')
      else
        return navigateTo(`/${server}/public/local`)
    }

    // Installed PWA shortcut to local
    if (to.query['local-pwa-shortcut'] !== undefined)
      return navigateTo(`/${server}/public/local`)
  }

  if (!currentUser.value) {
    if (to.path === '/home' && to.query['share-target'] !== undefined)
      return navigateTo('/share-target')
    else
      return navigateTo(`/${server}/public/local`)
  }

  if (to.path === '/')
    return navigateTo('/home')
}
