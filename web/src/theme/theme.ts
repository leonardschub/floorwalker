import { useSyncExternalStore } from 'react'

import {
 defaultThemePreference,
 isThemePreference,
 THEME_STORAGE_KEY,
 type ResolvedMode,
 type ThemePreference,
} from './preference'

const darkQuery = '(prefers-color-scheme: dark)'

const listeners = new Set<() => void>()

/* Set when storage refuses the write, so the choice still holds for this session. */
let inMemoryPreference: ThemePreference | null = null
let systemQuery: MediaQueryList | null = null
let started = false

function currentPreference(): ThemePreference {
 if (inMemoryPreference) return inMemoryPreference

 try {
  const stored = window.localStorage.getItem(THEME_STORAGE_KEY)
  return isThemePreference(stored) ? stored : defaultThemePreference
 } catch {
  return defaultThemePreference
 }
}

function resolveMode(preference: ThemePreference): ResolvedMode {
 if (preference !== 'system') return preference

 return window.matchMedia(darkQuery).matches ? 'dark' : 'light'
}

function notify() {
 for (const listener of listeners) listener()
}

function onSystemChange() {
 paint()
 notify()
}

function onStorage(event: StorageEvent) {
 if (event.key !== null && event.key !== THEME_STORAGE_KEY) return

 inMemoryPreference = null
 paint()
 notify()
}

function syncSystemListener() {
 const followsSystem = currentPreference() === 'system'

 if (followsSystem && !systemQuery) {
  systemQuery = window.matchMedia(darkQuery)
  systemQuery.addEventListener('change', onSystemChange)
  return
 }

 if (!followsSystem && systemQuery) {
  systemQuery.removeEventListener('change', onSystemChange)
  systemQuery = null
 }
}

/* The resolved mode reaches the DOM as both the class and the attribute, which is the
   kit's own contract and what the dark custom variant is defined against. */
function paint(): ResolvedMode {
 const mode = resolveMode(currentPreference())
 const root = document.documentElement

 root.classList.toggle('dark', mode === 'dark')
 root.classList.toggle('light', mode === 'light')
 root.dataset.theme = mode
 syncSystemListener()

 return mode
}

export function getPreference(): ThemePreference {
 return currentPreference()
}

export function resolvedMode(): ResolvedMode {
 return resolveMode(currentPreference())
}

export function setPreference(next: ThemePreference): void {
 inMemoryPreference = next

 try {
  window.localStorage.setItem(THEME_STORAGE_KEY, next)
 } catch {
  // This device holds the choice for the session only.
 }

 paint()
 notify()
}

export function subscribe(listener: () => void): () => void {
 startTheme()
 listeners.add(listener)

 return () => {
  listeners.delete(listener)
 }
}

/* Arms the resolver for the whole document: a system preference follows the operating
   system while the app is open, and another tab's choice applies here. */
export function startTheme(): void {
 if (started || typeof window === 'undefined') return

 started = true
 window.addEventListener('storage', onStorage)
 paint()
}

export function useThemePreference(): ThemePreference {
 return useSyncExternalStore(
  subscribe,
  getPreference,
  () => defaultThemePreference,
 )
}

export function useResolvedMode(): ResolvedMode {
 return useSyncExternalStore(
  subscribe,
  resolvedMode,
  () => defaultThemePreference,
 )
}
