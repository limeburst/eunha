// Reads window.__eunha_instance (injected by the axum server from the Host header)
// and pre-configures Elk's single-instance mode so the server selector is skipped.
export default defineNuxtPlugin(() => {
  const instance = (window as any).__eunha_instance as string | undefined
  if (!instance) return

  const config = useAppConfig() as any
  if (config.singleInstance !== undefined) {
    config.singleInstance = instance
  }

  // Also seed the current server so Elk skips the onboarding screen.
  const currentServer = useCookie('currentServer')
  if (!currentServer.value) {
    currentServer.value = instance
  }
})
