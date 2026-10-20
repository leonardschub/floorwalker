import { useEffect, useState } from 'react'

import { loadAgentsClient, type AgentsClient } from './client'

/** The build's agents client, loaded once for every page that shows the side panel.
    Null in a build that has none: a production browser build. */
export const agentsClientLoad = loadAgentsClient()

/** The agents client once its module has loaded, null until then and in a build that
    has none. */
export function useAgentsClient(): AgentsClient | null {
 const [client, setClient] = useState<AgentsClient | null>(null)

 useEffect(() => {
  if (agentsClientLoad === null) return
  let live = true
  void agentsClientLoad.then((loaded) => {
   if (live) setClient(loaded)
  })
  return () => {
   live = false
  }
 }, [])

 return client
}
