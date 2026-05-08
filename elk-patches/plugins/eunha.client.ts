// Reads the <meta name="eunha-instance"> tag injected by axum from the Host header
// and configures Elk's routing to use this instance as the public server.
export default defineNuxtPlugin({ enforce: 'post', setup() {
  const instance = document.querySelector('meta[name="eunha-instance"]')?.getAttribute('content')
  if (!instance) return

  // Override publicServer so auth middleware redirects to our instance, not defaultServer.
  publicServer.value = instance
} })
