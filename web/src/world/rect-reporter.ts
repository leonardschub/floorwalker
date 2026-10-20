import type { WorldRect } from './client'

/* Reports an element's rectangle in physical pixels (CSS px × devicePixelRatio),
   relative to the webview's top-left (spec: "Rectangle reporting"; the spike's
   ui/index.html). It reports on start, through a ResizeObserver, on window resize and
   whenever devicePixelRatio changes, and skips a report equal to the last one.
   Returns the function that stops reporting. */
export function watchRect(
 element: Element,
 report: (rect: WorldRect) => void,
): () => void {
 let last = ''
 const measure = () => {
  const box = element.getBoundingClientRect()
  const dpr = window.devicePixelRatio || 1
  const rect = {
   x: Math.round(box.left * dpr),
   y: Math.round(box.top * dpr),
   w: Math.round(box.width * dpr),
   h: Math.round(box.height * dpr),
  }
  const key = `${rect.x},${rect.y},${rect.w},${rect.h}`
  if (key === last) return
  last = key
  report(rect)
 }

 const observer = new ResizeObserver(measure)
 observer.observe(element)
 window.addEventListener('resize', measure)

 /* A resolution query matches only the current ratio, so it changes exactly when the
    ratio does; then it is replaced by one for the new ratio. */
 let ratio: MediaQueryList | null = null
 const watchRatio = () => {
  ratio = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`)
  ratio.addEventListener('change', onRatio, { once: true })
 }
 const onRatio = () => {
  measure()
  watchRatio()
 }
 watchRatio()
 measure()

 return () => {
  observer.disconnect()
  window.removeEventListener('resize', measure)
  ratio?.removeEventListener('change', onRatio)
 }
}
