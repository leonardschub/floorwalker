import { useEffect, useRef, useState } from 'react'

import type { AgentsClient } from './client'

/** How often an open statistics view asks again while opencodex's ledger grows. */
const AT_MOST_EVERY_MS = 5_000

/** How often an open view asks again even when nothing grew. Each request keeps the
    ledger watched for a lease of 60 s (desktop/src-tauri/agents ledger_watch.rs), so
    the watch stops soon after the last statistics view closes. */
const KEEP_WATCHING_MS = 30_000

/** A statistics view's answer: null until the first one arrives. */
export type Answer<T> = { value: T | null; failed: boolean }

/** Asks `request` for a statistics view's figures when the view opens and whenever
    `key` changes (the range, the filter), then again each time the client reports
    that opencodex's ledger grew, at most every 5 seconds (spec "The data contract"),
    and every 30 seconds regardless, which keeps the ledger watched while the view
    is open. The view mounts only while its tab is open, so it asks only then. The figures of
    an older request never replace a newer one's. */
export function useStatistics<T>(
 client: AgentsClient,
 key: string,
 request: () => Promise<T>,
): Answer<T> {
 // The latest `request`, so a re-request after the ledger grew asks for what the
 // view shows now; `key` says when that changed.
 const latest = useRef(request)
 useEffect(() => {
  latest.current = request
 })
 const [answer, setAnswer] = useState<Answer<T> & { key: string | null }>({
  key: null,
  value: null,
  failed: false,
 })

 useEffect(() => {
  let live = true
  let asked = 0
  let lastAskedAt = 0
  let pending: number | null = null
  let stop: (() => void) | null = null

  const ask = () => {
   pending = null
   lastAskedAt = Date.now()
   const mine = ++asked
   latest.current().then(
    (value) => {
     if (live && mine === asked) setAnswer({ key, value, failed: false })
    },
    () => {
     if (live && mine === asked)
      setAnswer((last) => ({ ...last, key, failed: true }))
    },
   )
  }

  ask()
  const keepWatching = window.setInterval(() => {
   if (pending === null) ask()
  }, KEEP_WATCHING_MS)
  void client
   .onLedgerGrew(() => {
    if (!live || pending !== null) return
    const wait = Math.max(0, lastAskedAt + AT_MOST_EVERY_MS - Date.now())
    pending = window.setTimeout(ask, wait)
   })
   .then((unsubscribe) => {
    if (live) stop = unsubscribe
    else unsubscribe()
   })

  return () => {
   live = false
   window.clearInterval(keepWatching)
   if (pending !== null) window.clearTimeout(pending)
   stop?.()
  }
 }, [client, key])

 // Figures for another range or filter are not shown as this one's.
 return answer.key === key
  ? { value: answer.value, failed: answer.failed }
  : { value: null, failed: false }
}
