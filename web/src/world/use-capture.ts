import { useCallback, useEffect, useRef, useState, type RefObject } from 'react'

import { watchCapture, type Capture } from './capture'
import { withWorldClient } from './world-client'

/* The world screen's cursor capture: while `enabled` (the world is running and on
   screen), a click in the world view captures the cursor and mouse movement turns the
   camera through the world contract's look, each button press while captured goes
   through its click (a right-click sits in), a right-click on the view while released
   goes to `onFreeRightClick` (leaving the seat), a click never captures while `seated`
   (it is the seated terminal's), and every capture change is reported
   through the world contract's captured, so the world walks only while captured.
   Leaving the world screen, for any reason, releases. `release` is for the screen's own release paths: Back and the account
   menu.

   Desktop build only, like the rest of the world (world-client.ts): the browser
   bundle carries no world client, and its world screen never runs. */
export function useCapture(
 view: RefObject<HTMLElement | null>,
 enabled: boolean,
 seated: boolean,
 onFreeRightClick: () => void,
): { captured: boolean; release: () => void } {
 const [captured, setCaptured] = useState(false)
 const capture = useRef<Capture | null>(null)
 const freeRightClick = useRef(onFreeRightClick)
 const seatedNow = useRef(seated)
 useEffect(() => {
  freeRightClick.current = onFreeRightClick
  seatedNow.current = seated
 }, [onFreeRightClick, seated])

 useEffect(() => {
  if (!enabled) return
  const end = withWorldClient((client, live) => {
   if (!view.current) return
   capture.current = watchCapture(
    view.current,
    (dx, dy) => void client.sendLook(dx, dy),
    (next) => {
     void client.reportCaptured(next)
     if (live()) setCaptured(next)
    },
    (button) => void client.sendClick(button),
    () => freeRightClick.current(),
    () => seatedNow.current,
   )
  })
  return () => {
   end()
   capture.current?.stop()
   capture.current = null
   setCaptured(false)
  }
 }, [view, enabled])

 const release = useCallback(() => capture.current?.release(), [])
 return { captured, release }
}
