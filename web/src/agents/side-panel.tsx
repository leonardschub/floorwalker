import { Tabs } from '@heroui/react'
import { useTranslation } from '@floorwalker/i18n'

import AgentTreeView from './agent-tree-view'
import type { AgentsClient, GoToDesk, SitIn, StartSession } from './client'
import MonthView from './month-view'
import OpencodexView from './opencodex-view'
import TokensView from './tokens-view'
import { useAgentTree } from './use-agent-tree'
import { useNewSession } from './use-new-session'

/* The side panel (spec "The panel (web)"): tabs across the top, Agents first and
   selected, and one view below that scrolls on its own. It fills its parent and
   brings no page landmark or h1 of its own, so it drops unchanged into the world
   screen's side-panel slot as well as onto its own route. Its data comes only through
   `client`; `onGoToDesk` hears which agent was pressed, and `onSitIn` (beside the
   world only) which agent's row was right-clicked. With `startSession` (beside the
   world only) each project heading has a + that starts a new session there and sits
   in it through `onSitIn` once it appears. */

const tabs = ['agents', 'month', 'tokens', 'opencodex'] as const

export default function SidePanel({
 client,
 onGoToDesk,
 onSitIn,
 startSession,
 seatedSessionId = null,
}: {
 client: AgentsClient
 onGoToDesk: (target: GoToDesk) => void
 /** A right-click on an agent's row: sit in it (the world screen). */
 onSitIn?: (target: SitIn) => void
 /** New session: starts `claude --bg` in a project (the world screen). */
 startSession?: StartSession
 /** The session the player sits in on the world screen: its root agent's row is
     highlighted and reads seated. */
 seatedSessionId?: string | null
}) {
 const { t } = useTranslation()
 const tree = useAgentTree(client)
 const newSession = useNewSession(startSession, onSitIn)

 return (
  <Tabs
   defaultSelectedKey="agents"
   className="flex h-full min-h-0 w-full flex-col gap-0 bg-surface text-foreground"
  >
   <div className="shrink-0 border-b border-separator p-1.5">
    <Tabs.ListContainer>
     <Tabs.List aria-label={t('agents.tabs')} className="w-full">
      {tabs.map((tab) => (
       <Tabs.Tab
        key={tab}
        id={tab}
        className="h-auto min-h-8 w-auto min-w-0 grow basis-[min-content] px-0.5 py-1 text-center text-xs leading-tight"
       >
        {t(`agents.tab.${tab}`)}
        <Tabs.Indicator />
       </Tabs.Tab>
      ))}
     </Tabs.List>
    </Tabs.ListContainer>
   </div>
   {tabs.map((tab) => (
    <Tabs.Panel
     key={tab}
     id={tab}
     className="min-h-0 flex-1 overflow-y-auto p-3"
    >
     {tab === 'agents' ? (
      <AgentTreeView
       tree={tree}
       onGoToDesk={onGoToDesk}
       onSitIn={onSitIn}
       newSession={newSession}
       seatedSessionId={seatedSessionId}
      />
     ) : tab === 'tokens' ? (
      <TokensView client={client} onGoToDesk={onGoToDesk} />
     ) : tab === 'month' ? (
      <MonthView client={client} />
     ) : (
      <OpencodexView client={client} />
     )}
    </Tabs.Panel>
   ))}
  </Tabs>
 )
}
