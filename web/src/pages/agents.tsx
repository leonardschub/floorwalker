import { useEffect } from 'react'
import { useNavigate } from 'react-router'

import { useTranslation } from '@floorwalker/i18n'

import SidePanel from '../agents/side-panel'
import { agentsClientLoad, useAgentsClient } from '../agents/use-agents-client'
import { useDocumentTitle } from '../components/document-title'
import Notice from '../components/notice'
import { landingPath } from '../landing'

/* The side panel on its own route, filling the page (spec "The standalone route"). On
   the desktop the world screen shows the same panel in its side-panel slot. Which
   agents client it runs on is settled at build time (agents/client.ts): the Rust
   registry's in the desktop build, the fixtures in browser development. A production
   browser build has neither and leaves for where the browser build opens. Pressing an agent does nothing more here. */

function ignoreGoToDesk() {}

function Agents() {
 const { t } = useTranslation()
 useDocumentTitle(t('title.agents'))
 const navigate = useNavigate()
 const client = useAgentsClient()

 useEffect(() => {
  if (
   agentsClientLoad === null &&
   import.meta.env.MODE !== 'desktop' &&
   !import.meta.env.DEV
  )
   navigate(landingPath, { replace: true })
 }, [navigate])

 return (
  <main className="flex h-screen w-full flex-col overflow-hidden bg-background">
   <h1 className="sr-only">{t('title.agents')}</h1>
   {client ? (
    <SidePanel client={client} onGoToDesk={ignoreGoToDesk} />
   ) : (
    <div className="p-4">
     <Notice>{t('loading')}</Notice>
    </div>
   )}
  </main>
 )
}

export default Agents
