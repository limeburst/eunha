// Reads window.__eunha_instance (injected by the axum server from the Host header)
// and pre-configures Elk's routing to use this instance as the public server.
export default defineNuxtPlugin({ enforce: 'post', setup() {
  const instance = (window as any).__eunha_instance as string | undefined
  if (!instance) return

  // Override publicServer so auth middleware redirects to our instance, not defaultServer.
  publicServer.value = instance
} })
