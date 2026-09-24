import { defineConfig } from 'vite'
import { resolve } from 'path'

/**
 * Built from inside `web/` so it reuses the app's own TipTap install — the
 * spike is measuring the real editor, not a different copy of it. A shipping
 * version would be its own build target with its own dependencies.
 */
export default defineConfig({
  root: resolve(__dirname),
  base: './',
  resolve: {
    // Only the specifiers this entry names. A pattern would rewrite the
    // packages' own internal imports too, and pnpm does not hoist those.
    alias: ['core', 'starter-kit', 'extension-underline'].map((name) => ({
      find: `@tiptap/${name}`,
      replacement: resolve(__dirname, `../../../web/node_modules/@tiptap/${name}`),
    })),
  },
  build: {
    outDir: resolve(__dirname, 'dist'),
    emptyOutDir: true,
    // One file to ship into the app bundle, and one file to load offline.
    assetsInlineLimit: 100_000_000,
    rollupOptions: { output: { inlineDynamicImports: true } },
  },
})
