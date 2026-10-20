import { defineConfig, mergeConfig } from 'vitest/config'

import viteConfig from './vite.config.ts'

/* Two projects. The pages run as the desktop build (vite --mode desktop) in jsdom,
   with the shell faked at its IPC boundary (@tauri-apps/api/mocks), so a test sees
   what the app does and every command it asks the shell for. The node scripts under
   scripts/ (the legal validator) run in plain node. */
export default mergeConfig(
 viteConfig,
 defineConfig({
  test: {
   projects: [
    {
     extends: true,
     test: {
      name: 'desktop',
      mode: 'desktop',
      environment: 'jsdom',
      setupFiles: ['src/test/setup.ts'],
      /* A cold first load of a page's chunks takes a few seconds in jsdom. */
      testTimeout: 15_000,
      include: ['src/**/*.test.{ts,tsx}'],
     },
    },
    {
     extends: true,
     test: {
      name: 'scripts',
      environment: 'node',
      include: ['scripts/**/*.test.mjs'],
     },
    },
   ],
  },
 }),
)
