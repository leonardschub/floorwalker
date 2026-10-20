import { useCallback, useEffect, useRef, useState } from 'react'

import type { NewSessionFailure, SitIn, StartSession } from './client'

/* New session (agent-screens spec, "The side panel"): the + on a project heading
   starts a background session there; the shell waits for the agent tree to list it
   and answers with it, and the player sits in it. A start that fails, or no desk
   within the shell's deadline, leaves a failure under the heading, cleared by the next
   + on that project or after 30 s. The shell keeps the deadline, so the page and the
   shell never disagree on it. While a start is under way the project's + waits. */

/** How long a failure stays under its heading. */
const FAILURE_FOR_MS = 30_000

/* `all` without `key`. */
function without<T>(all: Record<string, T>, key: string): Record<string, T> {
 const rest = { ...all }
 delete rest[key]
 return rest
}

/* What the shell rejected with, as a failure: one of ours, or anything else (a
   denied command, a lost connection) as unknown, its text kept for the console. */
function failureOf(error: unknown): NewSessionFailure {
 if (
  typeof error === 'object' &&
  error !== null &&
  'error' in error &&
  typeof error.error === 'string'
 )
  return error as NewSessionFailure
 console.warn('[new session] the start failed on the way', error)
 return { error: 'unknown' }
}

export function useNewSession(
 start: StartSession | undefined,
 onSitIn: ((target: SitIn) => void) | undefined,
) {
 /* Each project's start under way, by the press that began it: an answer to an
    older press is ignored. */
 const [pending, setPendingState] = useState<Record<string, number>>({})
 /* The same, at once: an answer checks it before React renders. */
 const pendingNow = useRef<Record<string, number>>({})
 const setPending = useCallback(
  (next: (all: Record<string, number>) => Record<string, number>) => {
   pendingNow.current = next(pendingNow.current)
   setPendingState(pendingNow.current)
  },
  [],
 )
 const [failures, setFailures] = useState<Record<string, NewSessionFailure>>({})
 const presses = useRef(0)
 const timers = useRef(new Map<string, number>())

 const clearTimer = useCallback((projectId: string) => {
  window.clearTimeout(timers.current.get(projectId))
  timers.current.delete(projectId)
 }, [])

 useEffect(() => {
  const all = timers.current
  return () => {
   for (const timer of all.values()) window.clearTimeout(timer)
  }
 }, [])

 const startSession = useCallback(
  (projectId: string) => {
   if (!start || projectId in pendingNow.current) return
   const press = ++presses.current
   clearTimer(projectId)
   setFailures((all) => without(all, projectId))
   setPending((all) => ({ ...all, [projectId]: press }))
   /* Settles this press, unless a later one has taken over. */
   const settled = () => {
    if (pendingNow.current[projectId] !== press) return false
    setPending((all) => without(all, projectId))
    return true
   }
   start(projectId).then(
    (session) => {
     if (settled())
      onSitIn?.({
       sessionId: session.sessionId,
       agentId: session.sessionId,
       kind: 'root',
      })
    },
    (error: unknown) => {
     if (!settled()) return
     setFailures((all) => ({ ...all, [projectId]: failureOf(error) }))
     timers.current.set(
      projectId,
      window.setTimeout(() => {
       timers.current.delete(projectId)
       setFailures((all) => without(all, projectId))
      }, FAILURE_FOR_MS),
     )
    },
   )
  },
  [start, onSitIn, clearTimer, setPending],
 )

 return {
  /** Whether a start in this project is under way. */
  starting: (projectId: string) => projectId in pending,
  /** The failure under this project's heading, if any. */
  failure: (projectId: string): NewSessionFailure | null =>
   failures[projectId] ?? null,
  /** The + on this project's heading; nothing while a start there is under way. */
  start: start ? startSession : undefined,
 }
}
