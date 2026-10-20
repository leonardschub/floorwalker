import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

import react from '@vitejs/plugin-react'
import { defineConfig, type Plugin } from 'vite'
import tailwindcss from '@tailwindcss/vite'
import Pages from 'vite-plugin-pages'

import { THEME_STORAGE_KEY } from './src/theme/preference.ts'

const preloadSnippet = fileURLToPath(
 new URL('./src/theme/theme-preload.js', import.meta.url),
)

const storageKeyPlaceholder = '__THEME_STORAGE_KEY__'

/* Settles light, dark or system before first paint. The snippet is plain JS that lives
   beside the resolver, and the storage key it reads comes from the module the app reads
   it through, so the key is written once. */
function themePreload(): Plugin {
 return {
  name: 'ag-theme-preload',
  transformIndexHtml(html) {
   const snippet = readFileSync(preloadSnippet, 'utf8').replaceAll(
    storageKeyPlaceholder,
    THEME_STORAGE_KEY,
   )

   return {
    html,
    tags: [{ tag: 'script', children: snippet, injectTo: 'head-prepend' }],
   }
  },
 }
}

// https://vite.dev/config/
export default defineConfig({
 plugins: [
  react(),
  tailwindcss(),
  themePreload(),
  Pages({
   dirs: 'src/pages',
  }),
 ],
 server: { host: '127.0.0.1', port: 5173, strictPort: true },
})
