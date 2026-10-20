/* Runs before the module script, so a stored light preference never flashes dark.
   vite.config.ts inlines this file into index.html and substitutes the storage key. */
;(function () {
 try {
  var stored = window.localStorage.getItem('__THEME_STORAGE_KEY__')
  var preference =
   stored === 'light' || stored === 'dark' || stored === 'system'
    ? stored
    : 'dark'
  var mode =
   preference === 'system'
    ? window.matchMedia('(prefers-color-scheme: dark)').matches
      ? 'dark'
      : 'light'
    : preference
  var root = document.documentElement

  root.classList.toggle('dark', mode === 'dark')
  root.classList.toggle('light', mode === 'light')
  root.setAttribute('data-theme', mode)
 } catch {
  /* Storage is unavailable; the dark state index.html ships stands. */
 }
})()
