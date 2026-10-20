/* Cursor capture on the world view (spec: "Capture"; the spike's ui/index.html).

   A click in the world view captures the cursor through WebKit's Pointer Lock, and
   while captured, mouse movement is summed per animation frame and handed to `look`.
   Capture starts only inside the handler of a trusted mouse pointer event with the
   primary button: script-dispatched events, element.click() and keyboard activation
   never request the lock. That rule is the guard against WebKitGTK aborting the whole
   app when Pointer Lock is requested on a seat without a pointer (spike problem 2), so
   nothing else in the app calls requestPointerLock(), and nothing re-captures.

   While captured, each trusted primary-button press goes to `click` (management-rooms
   spec: "Clicks reach the world"): WebKitGTK still delivers mouse buttons under Pointer
   Lock. The press that captures comes before the lock, so it is never sent. A
   secondary-button press goes to `click` too, as 'secondary' (sit in: a
   right-click sits in the robot aimed at); one on the world view while released goes
   to `freeRightClick` (seated, it leaves). The webview's context menu never opens on
   the world view. While `seated()`, a click never captures: the seated terminal's
   mouse and keys are the session's.

   A press that moves before it is let go is a drag: it neither captures nor turns the
   camera (drag-to-turn is not shipped). Esc and focus moving to another window
   release; so does the returned stop function, which the world screen calls when it
   unmounts. `click` hears each press while captured. `onCaptured` hears every
   Pointer Lock change and error, and a last `false` from stop, whose release comes
   after the listeners are gone. Returns { release, stop }. */

/** How far (CSS px) a press may move and still count as a click. */
const DRAG_THRESHOLD = 4

/** Whether an event may start capture: a trusted primary-button mouse pointer event. */
export const mayCapture = (event: PointerEvent) =>
 event.isTrusted && event.pointerType === 'mouse' && event.button === 0

export type Capture = {
 /** Releases the cursor if this view holds it. */
 release: () => void
 /** Stops watching, and releases. */
 stop: () => void
}

export function watchCapture(
 view: HTMLElement,
 look: (dx: number, dy: number) => void,
 onCaptured: (captured: boolean) => void,
 click: (button: 'primary' | 'secondary') => void,
 freeRightClick: () => void,
 seated: () => boolean,
): Capture {
 const captured = () => document.pointerLockElement === view
 let press: { id: number; x: number; y: number; moved: boolean } | null = null
 let dx = 0
 let dy = 0
 let frame = 0

 const release = () => {
  /* Only the world view ever holds the lock, so any holder is released: by unmount
     time the view may already be out of the document. */
  if (document.pointerLockElement) document.exitPointerLock()
 }

 const onPointerDown = (event: PointerEvent) => {
  if (!mayCapture(event) || captured()) return
  press = {
   id: event.pointerId,
   x: event.clientX,
   y: event.clientY,
   moved: false,
  }
 }
 const onPointerMove = (event: PointerEvent) => {
  if (!press || press.id !== event.pointerId) return
  const moved = Math.hypot(event.clientX - press.x, event.clientY - press.y)
  if (moved >= DRAG_THRESHOLD) press.moved = true
 }
 const onPointerUp = (event: PointerEvent) => {
  const clicked = press && press.id === event.pointerId && !press.moved
  press = null
  if (!clicked || !mayCapture(event) || captured() || seated()) return
  try {
   const request = view.requestPointerLock() as unknown
   if (request instanceof Promise) request.catch(() => onCaptured(captured()))
  } catch {
   onCaptured(captured())
  }
 }
 const onPointerCancel = () => {
  press = null
 }

 const flush = () => {
  frame = 0
  if (dx || dy) look(dx, dy)
  dx = 0
  dy = 0
 }
 const onMouseMove = (event: MouseEvent) => {
  if (!captured()) return
  dx += event.movementX
  dy += event.movementY
  if (!frame) frame = requestAnimationFrame(flush)
 }

 const onMouseDown = (event: MouseEvent) => {
  if (!event.isTrusted || (event.button !== 0 && event.button !== 2)) return
  if (!captured()) return
  /* The movement summed so far turns the camera first, so the click aims where the
     player sees the dot. */
  if (frame) {
   cancelAnimationFrame(frame)
   flush()
  }
  click(event.button === 2 ? 'secondary' : 'primary')
 }
 /* A right-click on the world view, the seated overlay included, while released:
    caught before anything in the view hears it. */
 const onViewMouseDown = (event: MouseEvent) => {
  if (!event.isTrusted || event.button !== 2 || captured()) return
  event.preventDefault()
  event.stopPropagation()
  freeRightClick()
 }
 const onContextMenu = (event: Event) => event.preventDefault()

 const onLockChange = () => onCaptured(captured())
 const onKeyDown = (event: KeyboardEvent) => {
  if (event.key === 'Escape') release()
 }

 view.addEventListener('pointerdown', onPointerDown)
 view.addEventListener('pointermove', onPointerMove)
 view.addEventListener('pointerup', onPointerUp)
 view.addEventListener('pointercancel', onPointerCancel)
 view.addEventListener('mousedown', onViewMouseDown, { capture: true })
 view.addEventListener('contextmenu', onContextMenu)
 document.addEventListener('mousemove', onMouseMove)
 document.addEventListener('mousedown', onMouseDown)
 document.addEventListener('pointerlockchange', onLockChange)
 document.addEventListener('pointerlockerror', onLockChange)
 document.addEventListener('keydown', onKeyDown)
 window.addEventListener('blur', release)

 return {
  release,
  stop: () => {
   view.removeEventListener('pointerdown', onPointerDown)
   view.removeEventListener('pointermove', onPointerMove)
   view.removeEventListener('pointerup', onPointerUp)
   view.removeEventListener('pointercancel', onPointerCancel)
   view.removeEventListener('mousedown', onViewMouseDown, { capture: true })
   view.removeEventListener('contextmenu', onContextMenu)
   document.removeEventListener('mousemove', onMouseMove)
   document.removeEventListener('mousedown', onMouseDown)
   document.removeEventListener('pointerlockchange', onLockChange)
   document.removeEventListener('pointerlockerror', onLockChange)
   document.removeEventListener('keydown', onKeyDown)
   window.removeEventListener('blur', release)
   if (frame) cancelAnimationFrame(frame)
   release()
   onCaptured(false)
  },
 }
}
