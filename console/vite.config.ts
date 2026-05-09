import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { lingui } from '@lingui/vite-plugin'

export default defineConfig({
  plugins: [
    react({
      babel: {
        plugins: ['macros'],
      },
    }),
    lingui(),
  ],
  server: {
    port: 5174,
    proxy: {
      '/api': {
        target: 'http://localhost:3000',
        changeOrigin: false,
        headers: { host: 'local.eunha.social' },
      },
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
})
