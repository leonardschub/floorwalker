import '@floorwalker/i18n'

/* What jsdom leaves out and the pages use: the world view's rectangle is watched with
   a ResizeObserver and a device-pixel-ratio media query (world/rect-reporter.ts). A
   test never resizes or zooms anything, so stand-ins that never report are enough. */
class StillResizeObserver {
 observe() {}
 unobserve() {}
 disconnect() {}
}

globalThis.ResizeObserver ??= StillResizeObserver

window.matchMedia ??= (query: string) =>
 ({
  matches: false,
  media: query,
  onchange: null,
  addEventListener() {},
  removeEventListener() {},
  addListener() {},
  removeListener() {},
  dispatchEvent: () => false,
 }) as MediaQueryList
