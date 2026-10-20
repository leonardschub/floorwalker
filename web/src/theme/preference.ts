export const THEME_STORAGE_KEY = 'ag-theme'

export const themePreferences = ['light', 'dark', 'system'] as const

export type ThemePreference = (typeof themePreferences)[number]

export type ResolvedMode = 'light' | 'dark'

/* The key is absent, unrecognized or unreadable on most first visits, and the app is
   dark-first: dark is the preference then, not system. */
export const defaultThemePreference = 'dark' satisfies ThemePreference

export function isThemePreference(
 value: string | null,
): value is ThemePreference {
 return value === 'light' || value === 'dark' || value === 'system'
}
