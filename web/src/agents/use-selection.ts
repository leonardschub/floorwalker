import { useState } from 'react'

import type { GoToDesk } from './client'

/** Which agent is selected in a view, and how pressing one goes to its desk. */
export type Selection = {
 isSelected: (sessionId: string, agentId: string) => boolean
 select: (target: GoToDesk) => void
 /** Outlines an agent without going to its desk: one sat in from its row. */
 outline: (target: GoToDesk) => void
}

/** The selection the Agents and Tokens views share in behaviour: pressing an agent
    outlines it and calls `onGoToDesk` with its identity (spec "Go to desk"). */
export function useSelection(
 onGoToDesk: (target: GoToDesk) => void,
): Selection {
 const [selected, setSelected] = useState<GoToDesk | null>(null)
 return {
  isSelected: (sessionId, agentId) =>
   selected?.sessionId === sessionId && selected.agentId === agentId,
  select: (target) => {
   setSelected(target)
   onGoToDesk(target)
  },
  outline: setSelected,
 }
}
