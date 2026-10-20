/* Where the app opens is decided once, at build time. The desktop build (vite --mode
   desktop) opens straight into the world. The browser build has no world, so it opens
   on Settings, its one page with something to do (the theme, About and the licences).
   Vite replaces MODE at build time, so the browser bundle's branch settles on Settings.
   Nothing here reads the running environment. */
export const desktopBuild = import.meta.env.MODE === 'desktop'

export const landingPath = desktopBuild ? '/world' : '/settings'
