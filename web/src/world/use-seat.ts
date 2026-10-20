import { useCallback, useEffect, useState } from 'react'

import type { WorldHint, WorldSeat } from './client'
import { withWorldClient } from './world-client'

/** How long a hint shows under the centre dot (agent-screens spec: "Hints"). */
const HINT_FOR = 3_000

/* The seated view as the world reports it (agent-screens spec: "The seated view"):
   the seat, `{ sessionId, rect }` while the player sits in a session and null
   otherwise, and the hint of the last sit-in that couldn't be, for about 3 s. While
   `enabled` (the world is running and on screen); null once it isn't. Desktop build
   only, like the rest of the world. */
export function useSeat(enabled: boolean): {
 seat: WorldSeat
 hint: WorldHint | null
 showHint: (hint: WorldHint) => void
} {
 const [seat, setSeat] = useState<WorldSeat>(null)
 const [hint, setHint] = useState<{ hint: WorldHint; at: number } | null>(null)

 useEffect(() => {
  if (!enabled) return
  const end = withWorldClient(async (client, live) => {
   const stops = await Promise.all([
    client.onWorldSeat((next) => {
     if (live()) setSeat(next)
    }),
    client.onWorldHint((next) => {
     if (live()) setHint({ hint: next, at: Date.now() })
    }),
   ])
   return () => stops.forEach((stop) => stop())
  })
  return () => {
   end()
   setSeat(null)
   setHint(null)
  }
 }, [enabled])

 /* Each hint shows for HINT_FOR from its own arrival. */
 useEffect(() => {
  if (!hint) return
  const timer = window.setTimeout(
   () => setHint(null),
   Math.max(0, hint.at + HINT_FOR - Date.now()),
  )
  return () => window.clearTimeout(timer)
 }, [hint])

 const showHint = useCallback(
  (next: WorldHint) => setHint({ hint: next, at: Date.now() }),
  [],
 )
 return { seat, hint: hint?.hint ?? null, showHint }
}
