import { useEffect, useState } from 'react'

import type { AgentsClient, AgentTreeSnapshot } from './client'

const loading: AgentTreeSnapshot = { status: 'loading', projects: [] }

/** The agent tree from `client`: a snapshot on mount, then every change it reports.
    Reads 'loading' until the first answer. A change that arrives before the snapshot
    answers wins, since the snapshot may be older than it. */
export function useAgentTree(client: AgentsClient): AgentTreeSnapshot {
 const [tree, setTree] = useState(loading)

 useEffect(() => {
  let live = true
  let changed = false
  let stop: (() => void) | null = null

  void client
   .onSnapshot((next) => {
    if (!live) return
    changed = true
    setTree(next)
   })
   .then((unsubscribe) => {
    if (live) stop = unsubscribe
    else unsubscribe()
   })
  void client.snapshot().then((next) => {
   if (live && !changed) setTree(next)
  })

  return () => {
   live = false
   stop?.()
  }
 }, [client])

 return tree
}
