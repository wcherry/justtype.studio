import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { execSync } from 'node:child_process'
import { readFileSync } from 'node:fs'

/**
 * What the UI shows as "the version you are running" — see `src/lib/version.ts`.
 *
 * The numbers come from `web/version.txt` (`version=` / `commit=`), which the
 * Dockerfile's web stage generates per image — see `scripts/write-version.mjs`.
 * That file is deliberately not in the repo, so a build outside a container
 * has no version and is named by whatever commit git will give.
 */
function readVersionFile(): { version: string; commit: string } | null {
  let text: string
  try {
    text = readFileSync(new URL('./version.txt', import.meta.url), 'utf8')
  } catch {
    return null
  }
  const field = (key: string) => new RegExp(`^${key}=(.*)$`, 'm').exec(text)?.[1].trim() ?? ''
  return { version: field('version'), commit: field('commit') }
}

/** A local build is not a release, but the commit is one cheap command away. */
function commitFromGit(): string {
  try {
    return execSync('git rev-parse --short HEAD', { stdio: ['ignore', 'pipe', 'ignore'] }).toString().trim()
  } catch {
    return ''
  }
}

const { version, commit } = readVersionFile() ?? { version: '', commit: commitFromGit() }

export default defineConfig({
  plugins: [react()],
  define: {
    __APP_VERSION__: JSON.stringify(version),
    __BUILD_ID__: JSON.stringify(commit),
  },
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
