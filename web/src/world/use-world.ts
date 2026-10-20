import { useCallback, useEffect, useRef, useState, type RefObject } from 'react'

import type { StartFailure, WorldStatus } from './client'
import { watchRect } from './rect-reporter'
import { getWorldSettings } from './settings'
import { watchWorldStrings } from './strings'
import {
 loadWorldClient,
 withWorldClient,
 type WorldClient,
} from './world-client'

/** What the world screen shows in the world view. */
export type WorldScreenState = 'starting' | 'running' | 'failed' | 'stopped'

/* What this page last learned, so re-entering the world screen while the world runs
   shows it on the first paint instead of a flash of the starting state. The status
   read on mount corrects it either way. */
let lastKnown: WorldScreenState = 'starting'

/* Keeps lastKnown current while the world screen is not mounted too, so a world that
   crashed in the background paints "The world stopped" on the next entry's first
   frame, not a see-through view of a world that is gone. Installed once, by the first
   world screen, for the life of the page. */
let tracking = false
function track(client: WorldClient) {
 if (tracking) return
 tracking = true
 void client.onWorldStatus((status) => {
  lastKnown = screenStateOf(status)
 })
}

/* Counts world screens that left during a start and so cancelled it, so a start whose
   answer arrives after the cancel (its stop overtook it on the way to the shell) is
   stopped again. */
let cancelledStarts = 0

/* Stops the world completely (the world contract's stop): a start in progress is
   cancelled, a running world ends, and the next entry starts a fresh one with the
   starting state. Called by a world screen that leaves during a start. Does nothing in
   the browser build. */
async function endWorld(): Promise<void> {
 lastKnown = 'starting'
 await (await loadWorldClient())?.stopWorld()
}

/* The world screen's side of the world contract. As the screen mounts it listens for
   status changes, reports the world view's rectangle, tells the shell the world is
   shown (and, on unmount, that it no longer is: the world then simulates without
   drawing), and reads the status instead of assuming one: a running world shows at
   once (re-entry, or a reloaded webview), a world that crashed shows that it stopped,
   and otherwise it starts the world or joins the start in progress, which the shell
   turns into one world however often it is asked. A start that fails shows that the
   world couldn't start. `retry` (Try again) starts a fresh world from either notice.

   Leaving the world screen by any route (Settings, history Back) while a start is in
   progress cancels the start: the world draws only while shown, so a start left
   behind could never reach its first frame. A running world is kept warm. */
export function useWorld(view: RefObject<HTMLElement | null>): {
 shown: WorldScreenState
 retry: () => void
} {
 /* A failed start is retried on entry, so it paints the starting state first. */
 const [shown, setShown] = useState<WorldScreenState>(
  lastKnown === 'failed' ? 'starting' : lastKnown,
 )
 const begin = useRef<(() => Promise<void>) | null>(null)

 useEffect(() => {
  const cleanups: Array<() => void> = []
  /* Whether this screen has a start in flight. */
  let starting = false

  const end = withWorldClient(async (client, live) => {
   const show = (next: WorldScreenState) => {
    lastKnown = next
    if (live()) setShown(next)
   }
   track(client)
   /* The world starts with this device's options; Settings sends each change. */
   void client.reportWorldSettings(getWorldSettings())
   /* And with its text in the page's language, sent again on each change. */
   cleanups.push(watchWorldStrings(client))
   const unlisten = await client.onWorldStatus((status) =>
    show(screenStateOf(status)),
   )
   cleanups.push(unlisten)
   if (!live()) return void unlisten()
   if (view.current) {
    cleanups.push(
     watchRect(view.current, (rect) => void client.reportWorldRect(rect)),
    )
   }
   await client.reportWorldShown(true)
   if (!live()) return void client.reportWorldShown(false)
   cleanups.push(() => void client.reportWorldShown(false))
   begin.current = async () => {
    show('starting')
    const cancels = cancelledStarts
    starting = true
    try {
     await client.startWorld()
    } catch (token) {
     /* A stop cancelled the start, or the start failed; the status event has said
        so already, and a failure is shown even if that event was missed. */
     if ((token as StartFailure) !== 'world-cancelled') show('failed')
     return
    } finally {
     starting = false
    }
    if (!live() && cancels !== cancelledStarts) return void endWorld()
    show('running')
   }
   const status = await client.worldStatus()
   if (status === 'running' || status === 'stopped') return show(status)
   if (!live()) return
   await begin.current()
  })

  return () => {
   end()
   begin.current = null
   for (const cleanup of cleanups) cleanup()
   if (starting) {
    cancelledStarts += 1
    void endWorld()
   }
  }
 }, [view])

 /* The main window is transparent: every screen paints its own background, and html
    and body give way only while the running world is on screen (index.css). */
 useEffect(() => {
  if (shown !== 'running') return
  const root = document.documentElement
  root.classList.add('world-see-through')
  return () => root.classList.remove('world-see-through')
 }, [shown])

 const retry = useCallback(() => void begin.current?.(), [])
 return { shown, retry }
}

/* What the world view shows for a status the shell reports. */
function screenStateOf(status: WorldStatus): WorldScreenState {
 if (status === 'running' || status === 'failed' || status === 'stopped') {
  return status
 }
 return 'starting'
}
