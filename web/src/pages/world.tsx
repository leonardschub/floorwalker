import { lazy, Suspense, useCallback, useEffect, useRef } from 'react'
import { LuSettings } from 'react-icons/lu'
import { useNavigate } from 'react-router'

import { Button, Spinner } from '@heroui/react'
import { Trans, useTranslation } from '@floorwalker/i18n'

import type { AgentsClient, GoToDesk, SitIn } from '../agents/client'
import SidePanel from '../agents/side-panel'
import { useAgentTree } from '../agents/use-agent-tree'
import { useAgentsClient } from '../agents/use-agents-client'
import { useDocumentTitle } from '../components/document-title'
import { desktopBuild, landingPath } from '../landing'
import { useCapture } from '../world/use-capture'
import { useSeat } from '../world/use-seat'
import { useWorld, type WorldScreenState } from '../world/use-world'
import { loadWorldClient, type WorldClient } from '../world/world-client'

/* The world screen: the D frame around the world view (ticket 13, spec "The world
   screen route"), and where the app opens (local-only spec). A thin top bar holds the
   Settings button, a thin bottom bar is an empty surface, the side panel on the right
   shows the agents (the side-panel spec's component), and the world view fills the
   rest. Pressing an agent in the panel is go to desk: the player jumps beside its
   desk, without capturing the cursor. Nothing here asks who the user is: there is no
   account. The screen exists only in the desktop build: a browser visitor leaves for
   where the browser build opens.

   The world view shows the starting state (an opaque fill, a spinner, "Starting the
   world…") until the world has drawn its first frame, then turns transparent so the
   world, presented under the webview, shows through. A start that fails shows "The
   world couldn't start", and a world that crashed shows "The world stopped", both
   opaque and with Try again, which starts a fresh world. The world starts as the
   screen opens. Leaving the screen (Settings) leaves a running world running, and
   cancels a start in progress. While it runs, a click in the world view captures the
   cursor and mouse movement turns the camera (use-capture.ts); Settings and leaving
   the screen release it.

   Sitting in (ticket 12): while captured, a right-click on a robot or its desk within
   4 m seats the player at it. The world snaps the camera into the robot's eyes and
   reports the seat; the cursor is released, a terminal overlay is laid on the PC's
   glass (seated-terminal.tsx), the top bar reads "Seated at project · model" with
   Leave, and the agent's row in the panel reads seated. Every key goes to the session
   (ticket 13), and a click on the view never captures while seated. A
   right-click anywhere on the world view or Leave leaves; so does the session's attach
   leaving it (`/exit`, a double Ctrl+C or ← on an empty prompt). A sit-in that can't
   be shows its hint under the centre dot for about 3 s.

   From the side panel (ticket 14): right-clicking a root agent's row sits the player
   in it from anywhere, and from seat to seat while seated (Leave then goes back to
   where they first stood); a terminal-started one's row gets the terminal hint from
   the world, and a subagent's the nothing hint. The context-menu key or Shift+F10 on
   a focused row does what a right-click does. A left-click is still go to desk,
   which leaves the seat first. The panel stays usable while seated.

   New session (ticket 15): the + on a project heading starts a background session in
   the project's folder and, once the agent tree lists it, sits the player in it, from
   anywhere; a start that fails, or no desk by the shell's deadline, leaves a red line
   under the heading.

   Each region carries data-world-region so the acceptance drive can measure the frame
   without depending on its markup. */

/* The seated terminal overlay, in the desktop build only: the browser bundle carries
   neither xterm.js nor the font. The check is written out for the bundler, as in
   world-client.ts. */
const SeatedTerminal =
 import.meta.env.MODE === 'desktop'
  ? lazy(() => import('../world/seated-terminal'))
  : null

/* Asks the world client (desktop build only) without waiting for the answer; a
   failure is logged as `failed`. */
function tellWorld(
 failed: string,
 ask: (world: WorldClient) => Promise<unknown>,
) {
 void loadWorldClient()
  ?.then(ask)
  .catch((error: unknown) => console.warn(`[world] ${failed}`, error))
}

/* Leaving the seated view: Leave, or a right-click on the world view while seated. */
const leaveSeat = () =>
 tellWorld('could not leave the seat', (world) => world.leaveSeat())

/* The seated session's terminal output, for the overlay. */
const streamTerminal = (
 sessionId: string,
 onBytes: (bytes: Uint8Array) => void,
) =>
 tellWorld('no terminal stream', (world) =>
  world.streamTerminal(sessionId, onBytes),
 )

/* What the player types in the seated terminal, for the seated session. */
function typeIntoTerminal(sessionId: string, bytes: Uint8Array) {
 return (
  loadWorldClient()?.then((world) => world.terminalInput(sessionId, bytes)) ??
  Promise.resolve()
 )
}

/* A Ctrl+clicked link from the seated terminal, in the default browser. */
const openLink = (url: string) =>
 tellWorld('could not open a link', (world) => world.openLink(url))

/* "Seated at project · model", from the agent tree. */
function SeatedAt({
 client,
 sessionId,
}: {
 client: AgentsClient
 sessionId: string
}) {
 const { t } = useTranslation()
 const tree = useAgentTree(client)
 for (const project of tree.projects) {
  const session = project.sessions.find((s) => s.id === sessionId)
  if (session)
   return (
    <span className="min-w-0 truncate text-xs text-foreground">
     {session.root.model
      ? t('world.seated-at', {
         project: project.name,
         model: session.root.model,
        })
      : t('world.seated-at-project', { project: project.name })}
    </span>
   )
 }
 return null
}

/* The notice the world view shows in each state, if any. */
const noticeOf: Record<
 WorldScreenState,
 { heading: string; line: string } | null
> = {
 starting: null,
 running: null,
 failed: { heading: 'world.failed', line: 'world.failed-line' },
 stopped: { heading: 'world.stopped', line: 'world.stopped-line' },
}

/* Go to desk: the shell keeps a press made while the world starts and ignores one for
   an agent with no desk, so the page sends every press. */
const goToDesk = (target: GoToDesk) =>
 tellWorld('could not go to the desk', (world) => world.goToDesk(target))

/* Sit in from the side panel: only a world that exists hears it, and it answers a
   terminal-started session with its hint. */
const sitIn = (sessionId: string) =>
 tellWorld('could not sit in', (world) => world.sitIn(sessionId))

/* New session: `claude --bg` in the project's folder, through the shell. */
function startSession(projectId: string) {
 return (
  loadWorldClient()?.then((world) => world.newSession(projectId)) ??
  Promise.reject(new Error('no world client'))
 )
}

function World() {
 const { t } = useTranslation()
 useDocumentTitle(t('title.world'))
 const navigate = useNavigate()
 const view = useRef<HTMLElement>(null)
 const { shown: world, retry } = useWorld(view)
 const running = world === 'running'
 const worldNotice = noticeOf[world]
 const { seat, hint, showHint } = useSeat(running)
 const seated = seat !== null
 const onSitIn = useCallback(
  (target: SitIn) => {
   if (target.kind === 'root') sitIn(target.sessionId)
   else showHint('nothing')
  },
  [showHint],
 )
 const onFreeRightClick = useCallback(() => {
  if (seated) leaveSeat()
 }, [seated])
 const { captured, release } = useCapture(
  view,
  running,
  seated,
  onFreeRightClick,
 )
 const agents = useAgentsClient()

 /* Sitting in frees the cursor: the keys and the mouse are the terminal's. */
 useEffect(() => {
  if (seated) release()
 }, [seated, release])

 useEffect(() => {
  if (!desktopBuild) navigate(landingPath, { replace: true })
 }, [navigate])

 return (
  <div
   className={`flex h-screen w-full flex-col overflow-hidden text-foreground antialiased ${running ? 'bg-transparent' : 'bg-background'}`}
  >
   <header
    data-world-region="top-bar"
    className="flex h-8 shrink-0 items-center gap-2 border-b border-separator bg-surface px-1"
   >
    <h1 className="sr-only">{t('title.world')}</h1>
    {seat ? (
     <div
      data-world-region="seated"
      className="flex min-w-0 flex-1 items-center gap-2"
     >
      {agents ? <SeatedAt client={agents} sessionId={seat.sessionId} /> : null}
      <Button
       size="sm"
       variant="secondary"
       className="h-6 shrink-0 gap-1.5 rounded-full px-2.5 text-xs"
       onPress={leaveSeat}
      >
       <Trans
        i18nKey="world.leave-or-right-click"
        components={{ muted: <span className="text-muted" /> }}
       />
      </Button>
     </div>
    ) : (
     <div className="flex-1" />
    )}
    <Button
     size="sm"
     variant="ghost"
     className="h-6.5 gap-1 px-2 text-xs"
     onPress={() => {
      release()
      navigate('/settings')
     }}
    >
     <LuSettings aria-hidden="true" />
     {t('world.settings')}
    </Button>
   </header>
   <div className="flex min-h-0 flex-1">
    <main
     ref={view}
     data-world-region="view"
     data-world-state={world}
     data-world-captured={captured ? 'true' : 'false'}
     data-world-seated={seated ? 'true' : 'false'}
     className={`relative flex min-w-0 flex-1 flex-col items-center justify-center gap-3 ${running ? 'bg-transparent' : 'bg-background'}`}
    >
     {/* Always there, so a hint is announced as it arrives. */}
     <output
      data-world-hint={running && hint ? hint : undefined}
      className="pointer-events-none absolute top-[calc(50%+14px)] left-1/2 -translate-x-1/2"
     >
      {running && hint ? (
       <span className="block rounded-full bg-overlay/90 px-3 py-1 text-xs whitespace-nowrap text-foreground shadow-card">
        {t(`world.hint.${hint}`)}
       </span>
      ) : null}
     </output>
     {running && seat && SeatedTerminal ? (
      <Suspense fallback={null}>
       <SeatedTerminal
        key={seat.sessionId}
        seat={seat}
        stream={streamTerminal}
        input={typeIntoTerminal}
        openLink={openLink}
       />
      </Suspense>
     ) : null}
     {world === 'starting' ? (
      <>
       <Spinner size="md" aria-hidden="true" />
       <output className="text-sm text-muted">{t('world.starting')}</output>
      </>
     ) : worldNotice ? (
      <section
       role="alert"
       data-world-notice={world}
       className="flex w-full max-w-85 flex-col items-start gap-3 rounded-lg border border-separator bg-surface p-5.5"
      >
       <h2 className="text-base font-semibold">{t(worldNotice.heading)}</h2>
       <p className="text-sm text-muted">{t(worldNotice.line)}</p>
       <Button size="md" variant="primary" onPress={retry}>
        {t('world.try-again')}
       </Button>
      </section>
     ) : null}
    </main>
    <aside
     data-world-region="side-panel"
     className="flex w-1/5 shrink-0 border-l border-separator bg-surface"
    >
     {agents ? (
      <SidePanel
       client={agents}
       onGoToDesk={goToDesk}
       onSitIn={onSitIn}
       startSession={startSession}
       seatedSessionId={seat?.sessionId ?? null}
      />
     ) : null}
    </aside>
   </div>
   <div
    data-world-region="bottom-bar"
    className="h-8 shrink-0 border-t border-separator bg-surface"
   />
  </div>
 )
}

export default World
