import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { readFileSync } from 'node:fs'

const { version } = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8'))

export default defineConfig({
  plugins: [react()],
  define: { __APP_VERSION__: JSON.stringify(version) },
  server: {
    port: 5183,
    proxy: {
      // Keeps the browser on one origin in development, so no CORS preflight
      // and no separate base URL to configure.
      // `ws` so the live channel (/api/live) is proxied as a WebSocket too.
      '/api': { target: 'http://127.0.0.1:8088', ws: true },
    },
  },
})
