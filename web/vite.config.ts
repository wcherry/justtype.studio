import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5183,
    proxy: {
      // Keeps the browser on one origin in development, so no CORS preflight
      // and no separate base URL to configure.
      '/api': 'http://127.0.0.1:8088',
    },
  },
})
